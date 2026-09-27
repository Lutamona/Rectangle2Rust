//! URL-схема — порт `application(_:open:)` из `AppDelegate.swift`.
//!
//! - `rectangle2rust://execute-action?name=left-half` — выполнить действие.
//!   Имя — `name` действия (или старое `aliasName`) в kebab-case:
//!   `leftHalf` → `left-half`, `leftSide` → `left-side`, `columnFive1` → `column-five1`.
//! - `rectangle2rust://execute-task?name=ignore-app` / `unignore-app` —
//!   добавить приложение в список «Игнорировать» или убрать из него;
//!   `&app-bundle-id=com.apple.Safari` задаёт приложение, без него берётся активное.
//!   Ссылку может открыть любая страница, поэтому `app-bundle-id` должен быть
//!   похож на bundle id (латиница, цифры, `.`, `-`, `_`, до 255 знаков), иначе
//!   ссылка пропускается с записью в журнал — без вопроса пользователю.
//!
//! Схема задаётся в Info.plist (`CFBundleURLTypes`): `rectangle2rust`, у
//! dev-варианта — `rectangle2rust-dev`. Как и в оригинале, сама схема не
//! проверяется: обрабатывается любая ссылка, которую система отдала
//! приложению, а ссылки с путём (`…/execute-action/?name=…`) пропускаются.

use std::time::Duration;

use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSApplicationActivationOptions,
    NSRunningApplication, NSWorkspace,
};
use objc2_foundation::NSString;

use crate::actions::Action;
use crate::config::{self, Config};
use crate::events::AppInfo;
use crate::{app, app_delegate, events, log};

/// Что просит ссылка.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// `execute-action?name=…`
    ExecuteAction(Action),
    /// `execute-task?name=ignore-app|unignore-app[&app-bundle-id=…]`;
    /// `bundle_id` — значение `app-bundle-id`, если оно есть.
    Task {
        task: AppTask,
        bundle_id: Option<String>,
    },
}

/// Задачи `execute-task`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppTask {
    Ignore,
    Unignore,
}

impl AppTask {
    /// Имя задачи в ссылке.
    pub fn name(self) -> &'static str {
        match self {
            AppTask::Ignore => "ignore-app",
            AppTask::Unignore => "unignore-app",
        }
    }
}

// ---------------------------------------------------------------- разбор

/// Части ссылки, которые смотрит оригинал через `URLComponents`.
#[derive(Debug, PartialEq, Eq)]
struct UrlParts {
    host: Option<String>,
    path: String,
    /// Параметры запроса: имя и значение (`None` — параметр без `=`).
    query: Vec<(String, Option<String>)>,
}

impl UrlParts {
    /// Значение первого параметра с таким именем (`queryItems.first { … }?.value`).
    fn value(&self, name: &str) -> Option<String> {
        self.query
            .iter()
            .find(|(item, _)| item == name)
            .and_then(|(_, value)| value.clone())
    }
}

/// Разобрать ссылку по RFC 3986, как `URLComponents`: схема, `//хост`, путь,
/// `?запрос`, `#фрагмент`. `None` — это не ссылка (нет схемы).
fn split_url(url: &str) -> Option<UrlParts> {
    let (scheme, rest) = url.split_once(':')?;
    let scheme_is_valid = scheme.starts_with(|ch: char| ch.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'));
    if !scheme_is_valid {
        return None;
    }
    let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
    let (rest, query) = match rest.split_once('?') {
        Some((before, query)) => (before, Some(query)),
        None => (rest, None),
    };
    let (host, path) = match rest.strip_prefix("//") {
        Some(after) => {
            let end = after.find('/').unwrap_or(after.len());
            (Some(host_of(&after[..end])), &after[end..])
        }
        None => (None, rest),
    };
    let query = query
        .map(|query| {
            query
                .split('&')
                .map(|item| match item.split_once('=') {
                    Some((name, value)) => (percent_decode(name), Some(percent_decode(value))),
                    None => (percent_decode(item), None),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(UrlParts {
        host,
        path: percent_decode(path),
        query,
    })
}

/// Хост из `пользователь@хост:порт`.
fn host_of(authority: &str) -> String {
    let host_and_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if host_and_port.starts_with('[') {
        // IPv6: `[::1]:80`
        host_and_port
            .find(']')
            .map_or(host_and_port, |end| &host_and_port[..=end])
    } else {
        match host_and_port.rsplit_once(':') {
            Some((host, port)) if port.chars().all(|ch| ch.is_ascii_digit()) => host,
            _ => host_and_port,
        }
    };
    percent_decode(host)
}

/// `%XX` → байт; неправильные последовательности остаются как есть. `+` не
/// превращается в пробел (как у `URLComponents`).
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = |byte: u8| (byte as char).to_digit(16);
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                decoded.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// Имя действия в ссылке — `getUrlName` оригинала: перед заглавной буквой
/// дефис, сама буква — строчная.
pub fn url_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 4);
    for ch in name.chars() {
        if ch.is_uppercase() {
            result.push('-');
            result.extend(ch.to_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

/// Действие по имени из ссылки: первое из `WindowAction.active`, у которого
/// совпало старое имя (`aliasName`) или имя.
pub fn action_named(name: &str) -> Option<Action> {
    Action::active().iter().copied().find(|action| {
        action
            .alias_name()
            .is_some_and(|alias| url_name(alias) == name)
            || url_name(&action.name()) == name
    })
}

/// Разобрать ссылку. `None` — ссылку пропустить (оригинал делает то же молча).
pub fn parse(url: &str) -> Option<Command> {
    let parts = split_url(url)?;
    if !parts.path.is_empty() {
        return None;
    }
    let name = parts.value("name");
    match (parts.host.as_deref(), name.as_deref()) {
        (Some("execute-action"), name) => name.and_then(action_named).map(Command::ExecuteAction),
        (Some("execute-task"), Some("ignore-app")) => Some(Command::Task {
            task: AppTask::Ignore,
            bundle_id: parts.value("app-bundle-id"),
        }),
        (Some("execute-task"), Some("unignore-app")) => Some(Command::Task {
            task: AppTask::Unignore,
            bundle_id: parts.value("app-bundle-id"),
        }),
        _ => None,
    }
}

/// Приложение, к которому относится задача.
#[derive(Debug, PartialEq, Eq)]
enum TaskTarget {
    App(String),
    /// `app-bundle-id=` без значения — оригинал пишет об этом в журнал.
    Empty,
    /// `app-bundle-id` не похож на bundle id — в журнал и пропустить, без вопроса:
    /// иначе страница вписала бы свой текст в алерт, а после «Разрешить» — мусор в
    /// список «Игнорировать».
    Invalid,
    /// Параметра нет и активное приложение неизвестно — пропустить молча.
    Unknown,
}

/// Самое длинное имя, которое принимаем за bundle id.
const MAX_BUNDLE_ID_LEN: usize = 255;

/// Похоже на bundle id: латиница, цифры, `.`, `-`, `_` (и никаких управляющих
/// знаков), от 1 до 255 знаков.
fn is_valid_bundle_id(bundle_id: &str) -> bool {
    (1..=MAX_BUNDLE_ID_LEN).contains(&bundle_id.len())
        && bundle_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

/// `app-bundle-id` из ссылки, а без него — активное приложение. Проверяется
/// только то, что пришло в ссылке: bundle id активного приложения дала система.
fn task_target(given: Option<String>, front: Option<String>) -> TaskTarget {
    if given
        .as_deref()
        .is_some_and(|given| !given.is_empty() && !is_valid_bundle_id(given))
    {
        return TaskTarget::Invalid;
    }
    match given.or(front) {
        Some(bundle_id) if bundle_id.is_empty() => TaskTarget::Empty,
        Some(bundle_id) => TaskTarget::App(bundle_id),
        None => TaskTarget::Unknown,
    }
}

/// `ApplicationToggle.disableApp` / `enableApp`: список «Игнорировать» в настройках.
pub fn set_app_ignored(config: &mut Config, bundle_id: &str, ignored: bool) {
    let apps = config.disabled_apps.get_or_insert_with(Default::default);
    if ignored {
        apps.insert(bundle_id.to_string());
    } else {
        apps.remove(bundle_id);
    }
}

// ---------------------------------------------------------------- выполнение

/// Шаг и число шагов ожидания, пока фокус вернётся приложению пользователя.
const FOCUS_WAIT_STEP: Duration = Duration::from_millis(10);
const FOCUS_WAIT_STEPS: u32 = 50;

/// Ссылки, которые система отдала приложению (`application(_:open:)`).
pub fn open(urls: Vec<String>) {
    // Ссылку открыли без `open -g`, и система вывела приложение вперёд:
    // возвращаем фокус тому, с кем работал пользователь (`prevActiveApp?.activate()`),
    // иначе действие достанется нашему приложению, у которого нет окон.
    let focus_returned_to = if is_self_frontmost() {
        return_focus_to_user_app()
    } else {
        None
    };
    if let Some(app) = &focus_returned_to {
        log!(
            "Ссылка вывела приложение вперёд, фокус возвращается: {}",
            app.name.as_deref().unwrap_or("?")
        );
    }
    // Дальше — как `DispatchQueue.main.async` оригинала, только если фокус
    // возвращали, сначала ждём, пока он действительно вернётся: это асинхронно.
    // Сами ссылки — из таймера цикла событий, а не из блока главной очереди:
    // игнор по ссылке спрашивает подтверждение модальным алертом, а под ним
    // из блока очереди стояли бы обработчики мышиных мониторов.
    let steps = if focus_returned_to.is_some() {
        FOCUS_WAIT_STEPS
    } else {
        0
    };
    after_focus_returns(steps, move || {
        for url in &urls {
            handle(url);
        }
    });
}

fn after_focus_returns(steps_left: u32, then: impl FnOnce() + 'static) {
    if steps_left == 0 || !is_self_frontmost() {
        events::run_in_run_loop(then);
        return;
    }
    events::run_after(FOCUS_WAIT_STEP, move || {
        after_focus_returns(steps_left - 1, then)
    });
}

fn is_self_frontmost() -> bool {
    let own_pid = std::process::id() as i32;
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .is_some_and(|application| application.processIdentifier() == own_pid)
}

/// Активировать приложение, с которым работал пользователь; `None` — некого.
fn return_focus_to_user_app() -> Option<AppInfo> {
    let app = events::front_app_except_self()?;
    let application = NSRunningApplication::runningApplicationWithProcessIdentifier(app.pid)?;
    application
        .activateWithOptions(NSApplicationActivationOptions::empty())
        .then_some(app)
}

fn handle(url: &str) {
    log!("Ссылка: {url}");
    match parse(url) {
        Some(Command::ExecuteAction(action)) => app::execute(action),
        Some(Command::Task { task, bundle_id }) => run_task(task, bundle_id),
        None => log!("Ссылка не распознана: {url}"),
    }
}

fn run_task(task: AppTask, given: Option<String>) {
    let front = events::front_app_except_self().and_then(|app| app.bundle_id);
    let bundle_id = match task_target(given, front) {
        TaskTarget::App(bundle_id) => bundle_id,
        TaskTarget::Empty => {
            log!(
                "Пустой параметр app-bundle-id: передайте bundle id приложения или уберите параметр."
            );
            return;
        }
        TaskTarget::Invalid => {
            log!(
                "Параметр app-bundle-id не похож на bundle id (латиница, цифры, «.», «-», «_», до {MAX_BUNDLE_ID_LEN} знаков) — ссылка пропущена."
            );
            return;
        }
        TaskTarget::Unknown => return,
    };
    if !confirm_task(task, &bundle_id) {
        return;
    }
    config::update(|config| set_app_ignored(config, &bundle_id, task == AppTask::Ignore));
}

/// Спросить пользователя: такую ссылку может открыть любая страница или
/// программа, подставив чужой bundle id. Оригинал не спрашивает, если впереди
/// сам Rectangle, но ссылка, открытая без `open -g`, сама выводит приложение
/// вперёд, так что эта проверка ничего не доказывает, — здесь спрашиваем всегда.
fn confirm_task(task: AppTask, bundle_id: &str) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let name = app_delegate::app_name();
    let what = match task {
        AppTask::Ignore => "игнорировать",
        AppTask::Unignore => "перестать игнорировать",
    };
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&NSString::from_str(&format!(
        "Разрешить действие {name} по ссылке?"
    )));
    alert.setInformativeText(&NSString::from_str(&format!(
        "Внешний источник просит {name} {what} приложение «{bundle_id}» ({}). Разрешить?",
        task.name()
    )));
    alert.addButtonWithTitle(&NSString::from_str("Разрешить"));
    alert.addButtonWithTitle(&NSString::from_str("Отмена"));
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    alert.runModal() == NSAlertFirstButtonReturn
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn action(url: &str) -> Option<Action> {
        match parse(url) {
            Some(Command::ExecuteAction(action)) => Some(action),
            _ => None,
        }
    }

    #[test]
    fn url_names_are_kebab_case() {
        assert_eq!(url_name("leftHalf"), "left-half");
        assert_eq!(url_name("almostMaximize"), "almost-maximize");
        assert_eq!(url_name("columnFive1"), "column-five1");
        assert_eq!(url_name("displayOne"), "display-one");
        assert_eq!(url_name("maximize"), "maximize");
    }

    #[test]
    fn execute_action_by_name_and_alias() {
        assert_eq!(
            action("rectangle2rust://execute-action?name=left-half"),
            Some(Action::LeftHalf)
        );
        // Старые имена (`aliasName`).
        assert_eq!(
            action("rectangle2rust://execute-action?name=left-side"),
            Some(Action::LeftHalf)
        );
        assert_eq!(
            action("rectangle2rust://execute-action?name=center-section"),
            Some(Action::CenterHalf)
        );
        // Имена из README оригинала.
        assert_eq!(
            action("rectangle2rust://execute-action?name=center-prominently"),
            Some(Action::CenterProminently)
        );
        assert_eq!(
            action("rectangle2rust://execute-action?name=cascade-active-app"),
            Some(Action::CascadeActiveApp)
        );
        assert_eq!(
            action("rectangle2rust://execute-action?name=column-five1"),
            Some(Action::Column { count: 5, index: 1 })
        );
        assert_eq!(
            action("rectangle2rust://execute-action?name=display-two"),
            Some(Action::Display(2))
        );
    }

    #[test]
    fn every_active_action_has_a_working_url() {
        for &expected in Action::active() {
            let url = format!(
                "rectangle2rust://execute-action?name={}",
                url_name(&expected.name())
            );
            assert_eq!(action(&url), Some(expected), "{url}");
        }
    }

    #[test]
    fn scheme_is_not_checked() {
        for scheme in [
            "rectangle",
            "rectangle2",
            "rectangle2rust",
            "rectangle2rust-dev",
        ] {
            assert_eq!(
                action(&format!("{scheme}://execute-action?name=maximize")),
                Some(Action::Maximize),
                "{scheme}"
            );
        }
    }

    #[test]
    fn query_is_decoded_like_url_components() {
        assert_eq!(
            action("rectangle2rust://execute-action?name=left%2Dhalf"),
            Some(Action::LeftHalf)
        );
        // Первый параметр `name` решает, остальные не смотрятся.
        assert_eq!(
            action("rectangle2rust://execute-action?name=maximize&name=left-half"),
            Some(Action::Maximize)
        );
        assert_eq!(
            action("rectangle2rust://execute-action?foo=1&name=restore#fragment"),
            Some(Action::Restore)
        );
        // Хост с портом и пользователем.
        assert_eq!(
            action("rectangle2rust://user@execute-action:80?name=restore"),
            Some(Action::Restore)
        );
    }

    #[test]
    fn unknown_or_malformed_urls_are_skipped() {
        for url in [
            "rectangle2rust://execute-action?name=no-such-action",
            "rectangle2rust://execute-action?name=LEFT-HALF",
            "rectangle2rust://execute-action?name=leftHalf",
            "rectangle2rust://execute-action?name",
            "rectangle2rust://execute-action",
            // С путём — `components.path.isEmpty` не выполняется.
            "rectangle2rust://execute-action/?name=left-half",
            "rectangle2rust://execute-action/left-half",
            "rectangle2rust://Execute-Action?name=left-half",
            "rectangle2rust://execute-task?name=left-half",
            "rectangle2rust://execute-task?name=hide-app",
            "rectangle2rust:execute-action?name=left-half",
            "execute-action?name=left-half",
            "",
        ] {
            assert_eq!(parse(url), None, "{url}");
        }
    }

    #[test]
    fn tasks_with_and_without_bundle_id() {
        assert_eq!(
            parse("rectangle2rust://execute-task?name=ignore-app"),
            Some(Command::Task {
                task: AppTask::Ignore,
                bundle_id: None
            })
        );
        assert_eq!(
            parse("rectangle2rust://execute-task?name=unignore-app&app-bundle-id=com.apple.Safari"),
            Some(Command::Task {
                task: AppTask::Unignore,
                bundle_id: Some("com.apple.Safari".to_string())
            })
        );
        assert_eq!(
            parse("rectangle2rust://execute-task?app-bundle-id=&name=ignore-app"),
            Some(Command::Task {
                task: AppTask::Ignore,
                bundle_id: Some(String::new())
            })
        );
        // Параметр без `=` — значения нет, как `queryItem.value == nil`.
        assert_eq!(
            parse("rectangle2rust://execute-task?name=ignore-app&app-bundle-id"),
            Some(Command::Task {
                task: AppTask::Ignore,
                bundle_id: None
            })
        );
    }

    #[test]
    fn task_target_prefers_parameter_then_front_app() {
        let front = || Some("com.apple.Terminal".to_string());
        assert_eq!(
            task_target(Some("com.apple.Safari".to_string()), front()),
            TaskTarget::App("com.apple.Safari".to_string())
        );
        assert_eq!(
            task_target(None, front()),
            TaskTarget::App("com.apple.Terminal".to_string())
        );
        // Пустой параметр не заменяется активным приложением.
        assert_eq!(task_target(Some(String::new()), front()), TaskTarget::Empty);
        assert_eq!(task_target(None, None), TaskTarget::Unknown);
    }

    #[test]
    fn bundle_id_parameter_must_look_like_a_bundle_id() {
        let front = || Some("com.apple.Terminal".to_string());
        let target = |given: &str| task_target(Some(given.to_string()), front());
        for good in [
            "com.apple.Safari",
            "org.mozilla.firefox",
            "com.microsoft.VSCode",
            "com.company.my_app-2",
            "a",
        ] {
            assert_eq!(target(good), TaskTarget::App(good.to_string()), "{good}");
        }
        assert_eq!(target(&"a".repeat(255)), TaskTarget::App("a".repeat(255)));
        for bad in [
            "x\n\nЭто проверка безопасности",
            "com.apple.Safari\u{0}",
            "com.apple.Safari\t",
            "com apple Safari",
            "com.apple.Сафари",
            "com.apple.Safari»",
            "«com.apple.Safari",
            "com/apple/Safari",
        ] {
            assert_eq!(target(bad), TaskTarget::Invalid, "{bad:?}");
        }
        assert_eq!(target(&"a".repeat(256)), TaskTarget::Invalid);
        // Пустой — по-прежнему своё сообщение, без параметра — активное приложение.
        assert_eq!(target(""), TaskTarget::Empty);
        assert_eq!(
            task_target(None, Some("ru.keepcoder.Telegram".to_string())),
            TaskTarget::App("ru.keepcoder.Telegram".to_string())
        );
    }

    #[test]
    fn injected_text_in_a_task_link_is_skipped() {
        let url = "rectangle2rust://execute-task?name=ignore-app&app-bundle-id=x%0A%0A%D0%AD%D1%82%D0%BE%20%D0%BF%D1%80%D0%BE%D0%B2%D0%B5%D1%80%D0%BA%D0%B0";
        let Some(Command::Task { task, bundle_id }) = parse(url) else {
            panic!("ссылка разбирается как задача");
        };
        assert_eq!(task, AppTask::Ignore);
        assert_eq!(bundle_id.as_deref(), Some("x\n\nЭто проверка"));
        assert_eq!(task_target(bundle_id, None), TaskTarget::Invalid);

        // Действия над окнами — без вопроса, как в оригинале.
        assert_eq!(
            parse("rectangle2rust://execute-action?name=tile-all"),
            Some(Command::ExecuteAction(Action::TileAll))
        );
    }

    #[test]
    fn ignoring_and_unignoring_apps() {
        let mut config = Config::default();
        set_app_ignored(&mut config, "com.apple.Safari", true);
        set_app_ignored(&mut config, "com.apple.Terminal", true);
        assert!(config.is_app_disabled("com.apple.Safari"));
        set_app_ignored(&mut config, "com.apple.Safari", false);
        assert!(!config.is_app_disabled("com.apple.Safari"));
        set_app_ignored(&mut config, "com.apple.Terminal", false);
        // Как в оригинале: пустой список остаётся списком, а не «не задано».
        assert_eq!(config.disabled_apps, Some(BTreeSet::new()));
    }

    #[test]
    fn url_parts() {
        assert_eq!(
            split_url("rectangle2rust://execute-task?name=ignore-app&app-bundle-id=a%20b+c"),
            Some(UrlParts {
                host: Some("execute-task".to_string()),
                path: String::new(),
                query: vec![
                    ("name".to_string(), Some("ignore-app".to_string())),
                    ("app-bundle-id".to_string(), Some("a b+c".to_string())),
                ],
            })
        );
        assert_eq!(
            split_url("x://[::1]:8080/path"),
            Some(UrlParts {
                host: Some("[::1]".to_string()),
                path: "/path".to_string(),
                query: Vec::new(),
            })
        );
        assert_eq!(percent_decode("%D0%BF%D1%80%2"), "пр%2");
        assert_eq!(split_url("1x://host"), None);
    }
}
