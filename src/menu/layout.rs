//! Состав меню статус-бара как данные: пункты, разделители, подменю.
//!
//! Порт `AppDelegate.addWindowActionMenuItems` оригинала (`docs/ui-spec.md`
//! §2.3) плюс подменю «Другое» (решение D3), пункты Todo
//! (`addTodoModeMenuItems`, §2.4) и нижняя часть из storyboard (§2.2). AppKit
//! здесь нет: из этих данных `menu.rs` собирает `NSMenu` и при каждом
//! открытии прячет пункты по тем же правилам (`action_shown`, `service_shown`,
//! `collapse_separators`), а тесты сверяют состав с оригиналом.

use crate::actions::{Action, WindowActionCategory};
use crate::config::Config;
use crate::localization;

/// Настройки, от которых зависит состав меню; поменялись — меню собирается заново.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// `showAllActionsInMenu`: действия плоским списком, без подменю категорий.
    pub show_all_actions: bool,
    /// `showAdditionalSizesInMenu`: трети и «Размер» уходят в подменю,
    /// появляются восьмые, девятые, двенадцатые и шестнадцатые.
    pub show_additional_sizes: bool,
    /// `todo` («Показывать Todo режим в меню»): пункты Todo и todo-действия в «Другом».
    pub todo: bool,
}

impl Settings {
    /// Как `userEnabled` оригинала: включено, только если явно задано «да».
    pub fn from_config(config: &Config) -> Settings {
        Settings {
            show_all_actions: config.show_all_actions_in_menu == Some(true),
            show_additional_sizes: config.show_additional_sizes_in_menu == Some(true),
            todo: config.todo == Some(true),
        }
    }
}

/// Служебный пункт — не действие над окном.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    /// «Окна приложения столбиками» — все окна активного приложения в N столбиков
    /// (сверх оригинала, этап 3E).
    AppColumnsTile,
    /// «Держать окна «<приложение>» столбиками»; галочка — режим включён.
    AppColumnsKeep,
    /// «Включить режим Todo приложения»; галочка — режим включён.
    TodoMode,
    /// «Использовать <приложение> в качестве приложения Todo».
    TodoApp,
    /// «Использовать как окно Todo».
    TodoWindow,
    /// «Обновить положение Todo окна».
    TodoReflow,
    /// «Игнорировать <приложение>»; галочка — приложение игнорируется.
    IgnoreApp,
    /// «Настройки…».
    Settings,
    /// «О программе».
    About,
    /// «Просмотр журнала…» — показывается вместо «О программе», пока зажат ⌥.
    ViewLogging,
    /// «Проверить обновления…».
    CheckForUpdates,
    /// «Выход из …», ⌘Q.
    Quit,
}

impl Service {
    pub const ALL: [Service; 12] = [
        Service::AppColumnsTile,
        Service::AppColumnsKeep,
        Service::TodoMode,
        Service::TodoApp,
        Service::TodoWindow,
        Service::TodoReflow,
        Service::IgnoreApp,
        Service::Settings,
        Service::About,
        Service::ViewLogging,
        Service::CheckForUpdates,
        Service::Quit,
    ];

    /// Подпись. `front_app` — имя активного приложения, `app_name` — этого.
    pub fn title(self, front_app: Option<&str>, app_name: &str) -> String {
        let front_app = front_app.unwrap_or_default();
        match self {
            Service::AppColumnsTile => crate::app_columns::TILE_MENU_TITLE.to_string(),
            Service::AppColumnsKeep => crate::app_columns::keep_menu_title(front_app),
            Service::TodoMode => localization::TODO_MODE.to_string(),
            Service::TodoApp => localization::todo_app(front_app),
            Service::TodoWindow => localization::TODO_WINDOW.to_string(),
            Service::TodoReflow => localization::TODO_REFLOW.to_string(),
            Service::IgnoreApp => localization::ignore_app(front_app),
            Service::Settings => localization::SETTINGS.to_string(),
            Service::About => localization::ABOUT.to_string(),
            Service::ViewLogging => localization::VIEW_LOGGING.to_string(),
            Service::CheckForUpdates => localization::CHECK_FOR_UPDATES.to_string(),
            Service::Quit => localization::quit(app_name),
        }
    }

    /// Гаснет без активного окна, как пункты действий.
    pub fn needs_front_window(self) -> bool {
        matches!(self, Service::AppColumnsTile | Service::AppColumnsKeep)
    }
}

/// Пункт меню.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// Действие над окном; подпись — `localization::action_title`.
    Action(Action),
    Separator,
    /// Подменю категории; заголовок — `WindowActionCategory::display_name`.
    Submenu(WindowActionCategory, Vec<Item>),
    Service(Service),
}

/// Меню целиком, сверху вниз.
pub fn layout(settings: &Settings) -> Vec<Item> {
    let mut items = window_actions(settings);
    items.push(Item::Separator);
    if settings.todo {
        items.extend(
            [
                Service::TodoMode,
                Service::TodoApp,
                Service::TodoWindow,
                Service::TodoReflow,
            ]
            .map(Item::Service),
        );
        items.push(Item::Separator);
    }
    items.push(Item::Service(Service::IgnoreApp));
    items.push(Item::Separator);
    items.extend(
        [
            Service::Settings,
            Service::About,
            Service::ViewLogging,
            Service::CheckForUpdates,
            Service::Quit,
        ]
        .map(Item::Service),
    );
    items
}

/// Пункты действий — `addWindowActionMenuItems`: действия с подписью в порядке
/// `WindowAction.active`, разделитель перед первым в группе, подменю категорий
/// после плоских пунктов в порядке `menuOrder`. Сразу за раскладками столбиков —
/// «Окна приложения столбиками» (сверх оригинала), последним — «Другое».
fn window_actions(settings: &Settings) -> Vec<Item> {
    use WindowActionCategory::{Eighths, Ninths, Sixteenths, Size, Thirds, Twelfths};

    let mut items = Vec::new();
    let mut submenus: Vec<(WindowActionCategory, Vec<Item>)> = Vec::new();
    for &action in Action::active() {
        if action.display_name().is_none() {
            continue;
        }
        if let Some(category) = action.category().filter(|_| !settings.show_all_actions) {
            // Без дополнительных размеров трети и «Размер» — плоские пункты.
            let flat = matches!(category, Thirds | Size) && !settings.show_additional_sizes;
            if !flat {
                if !items.is_empty() && action.first_in_group() {
                    submenus.push((category, Vec::new()));
                }
                if let Some((_, submenu)) = submenus.last_mut() {
                    submenu.push(Item::Action(action));
                }
                continue;
            }
        }
        // Пока «Размер» не подменю, «Почти максимизировать» идёт одним блоком
        // с «Максимизировать».
        let separator = action.first_in_group()
            && !(action == Action::AlmostMaximize && !settings.show_additional_sizes);
        if !items.is_empty() && separator {
            items.push(Item::Separator);
        }
        items.push(Item::Action(action));
    }

    // Восьмые…шестнадцатые — только с дополнительными размерами (в оригинале
    // их пункты скрыты; меню собирается заново, когда настройка меняется).
    submenus.retain(|(category, _)| {
        settings.show_additional_sizes
            || !matches!(category, Eighths | Ninths | Twelfths | Sixteenths)
    });
    submenus.sort_by_key(|(category, _)| category.menu_order());
    if !submenus.is_empty() {
        items.push(Item::Separator);
        items.extend(
            submenus
                .into_iter()
                .map(|(category, items)| Item::Submenu(category, items)),
        );
    }

    items.push(Item::Separator);
    items.push(Item::Service(Service::AppColumnsTile));
    items.push(Item::Service(Service::AppColumnsKeep));
    items.push(Item::Separator);
    items.push(Item::Submenu(
        WindowActionCategory::Other,
        other_items(settings.todo),
    ));
    items
}

/// Подменю «Другое» (решение D3): действия, у которых в оригинале нет пункта
/// меню, группами по смыслу. Todo-действия — только при включённом Todo.
pub fn other_items(todo: bool) -> Vec<Item> {
    use Action::*;

    let mut groups: Vec<Vec<Action>> = vec![
        vec![
            TopVerticalThird,
            MiddleVerticalThird,
            BottomVerticalThird,
            TopVerticalTwoThirds,
            BottomVerticalTwoThirds,
        ],
        vec![
            TopLeftThird,
            TopRightThird,
            BottomLeftThird,
            BottomRightThird,
        ],
        vec![CenterProminently, Specified],
        vec![LargerWidth, SmallerWidth, LargerHeight, SmallerHeight],
        vec![
            DoubleWidthLeft,
            DoubleWidthRight,
            DoubleHeightUp,
            DoubleHeightDown,
        ],
        vec![
            HalveWidthLeft,
            HalveWidthRight,
            HalveHeightUp,
            HalveHeightDown,
        ],
        (1..=9).map(Display).collect(),
        vec![
            TileAll,
            CascadeAll,
            TileActiveApp,
            CascadeActiveApp,
            ReverseAll,
        ],
    ];
    if todo {
        groups.push(vec![LeftTodo, RightTodo]);
    }

    let mut items = Vec::new();
    for group in groups {
        if !items.is_empty() {
            items.push(Item::Separator);
        }
        items.extend(group.into_iter().map(Item::Action));
    }
    items
}

// ---------------------------------------------------------------- видимость

/// Экраны на момент открытия меню.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Displays {
    /// `NSScreen.screens.count`.
    pub count: usize,
    /// «Считать экраны одним» (`combinedDisplayMode`).
    pub combined: bool,
}

/// Виден ли пункт действия. «Следующий/предыдущий экран» — при нескольких
/// экранах, если они не считаются одним (`updateWindowActionMenuItems`);
/// «Экран N» — по тому же правилу и только для существующих экранов.
pub fn action_shown(action: Action, displays: Displays) -> bool {
    let several = displays.count > 1 && !displays.combined;
    match action {
        Action::NextDisplay | Action::PreviousDisplay => several,
        Action::Display(number) => several && usize::from(number) <= displays.count,
        _ => true,
    }
}

/// Активное приложение на момент открытия меню.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrontApp<'a> {
    /// Имя (`ApplicationToggle.frontAppName`); неизвестно — пункты с именем скрыты.
    pub name: Option<&'a str>,
    /// Это Todo-приложение (`todoAppIsActive`).
    pub is_todo_app: bool,
    /// Его активное окно — Todo-окно (`TodoManager.isTodoWindowFront`).
    pub todo_window_front: bool,
}

/// Виден ли служебный пункт (`menuWillOpen`, `updateTodoModeMenuItems`).
pub fn service_shown(service: Service, front_app: &FrontApp) -> bool {
    match service {
        Service::IgnoreApp | Service::TodoApp | Service::AppColumnsKeep => front_app.name.is_some(),
        Service::TodoWindow => front_app.is_todo_app && !front_app.todo_window_front,
        _ => true,
    }
}

/// Итоговая видимость пунктов: `(разделитель ли, виден ли по своему правилу)`
/// → видимость. Разделитель не показывается первым и последним и сразу после
/// другого разделителя — спрятанные пункты не оставляют двойных черт.
pub fn collapse_separators(entries: impl IntoIterator<Item = (bool, bool)>) -> Vec<bool> {
    let mut visible = Vec::new();
    let mut after_separator = true;
    let mut trailing_separator = None;
    for (is_separator, shown) in entries {
        let show = shown && !(is_separator && after_separator);
        if show {
            after_separator = is_separator;
            trailing_separator = is_separator.then_some(visible.len());
        }
        visible.push(show);
    }
    if let Some(index) = trailing_separator {
        visible[index] = false;
    }
    visible
}

/// Меню текстом, как его видно при открытии: пункты сверху вниз, подменю —
/// строкой «Заголовок ▸» и пунктами с отступом (до глубины `depth`),
/// разделитель — «—». Для тестов и отладки.
pub fn render(
    items: &[Item],
    displays: Displays,
    front_app: &FrontApp,
    app_name: &str,
    depth: usize,
) -> String {
    let mut text = String::new();
    render_into(&mut text, items, displays, front_app, app_name, depth, 0);
    text
}

fn render_into(
    text: &mut String,
    items: &[Item],
    displays: Displays,
    front_app: &FrontApp,
    app_name: &str,
    depth: usize,
    level: usize,
) {
    let shown = collapse_separators(items.iter().map(|item| match item {
        Item::Action(action) => (false, action_shown(*action, displays)),
        Item::Separator => (true, true),
        Item::Submenu(..) => (false, true),
        Item::Service(service) => (false, service_shown(*service, front_app)),
    }));
    let indent = "  ".repeat(level);
    for (item, _) in items.iter().zip(shown).filter(|(_, shown)| *shown) {
        let line = match item {
            Item::Action(action) => localization::action_title(*action)
                .unwrap_or_default()
                .to_string(),
            Item::Separator => "—".to_string(),
            Item::Submenu(category, _) => format!("{} ▸", category.display_name()),
            Item::Service(Service::ViewLogging) => {
                format!(
                    "{} (⌥)",
                    Service::ViewLogging.title(front_app.name, app_name)
                )
            }
            Item::Service(Service::Quit) => {
                format!("{} ⌘Q", Service::Quit.title(front_app.name, app_name))
            }
            Item::Service(service) => service.title(front_app.name, app_name),
        };
        text.push_str(&indent);
        text.push_str(&line);
        text.push('\n');
        if let Item::Submenu(_, children) = item {
            if level < depth {
                render_into(
                    text,
                    children,
                    displays,
                    front_app,
                    app_name,
                    depth,
                    level + 1,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = "Rectangle 2 (Rust)";
    const ONE_DISPLAY: Displays = Displays {
        count: 1,
        combined: false,
    };
    const TWO_DISPLAYS: Displays = Displays {
        count: 2,
        combined: false,
    };
    const TERMINAL: FrontApp = FrontApp {
        name: Some("Терминал"),
        is_todo_app: false,
        todo_window_front: false,
    };

    fn menu(settings: Settings, displays: Displays, depth: usize) -> String {
        render(&layout(&settings), displays, &TERMINAL, APP, depth)
    }

    fn submenu(settings: Settings, category: WindowActionCategory) -> Vec<Item> {
        layout(&settings)
            .into_iter()
            .find_map(|item| match item {
                Item::Submenu(found, items) if found == category => Some(items),
                _ => None,
            })
            .unwrap_or_else(|| panic!("нет подменю {category:?}"))
    }

    fn actions(items: &[Item]) -> Vec<Action> {
        items
            .iter()
            .filter_map(|item| match item {
                Item::Action(action) => Some(*action),
                _ => None,
            })
            .collect()
    }

    /// Столбики приложения и «Другое» — сразу за последним подменю столбиков.
    const APP_COLUMNS_AND_OTHER: &str = "\
—
Окна приложения столбиками
Держать окна «Терминал» столбиками
—
Другое ▸
";

    /// Нижняя часть меню, одинаковая во всех тестах без Todo.
    const BOTTOM: &str = "\
—
Игнорировать Терминал
—
Настройки…
О программе
Просмотр журнала… (⌥)
Проверить обновления…
Выход из Rectangle 2 (Rust) ⌘Q
";

    #[test]
    fn default_menu_matches_the_original() {
        let expected = "\
Левая половина
Правая половина
Центральная половина
Верхняя половина
Нижняя половина
—
Слева вверху
Справа вверху
Внизу слева
Внизу справа
—
Первая треть
Центральная треть
Последняя треть
Первые две трети
Центральные две трети
Последние две трети
—
Максимизировать
Почти максимизировать
Максимизировать высоту
Увеличить
Уменьшить
В центр
Восстановить
—
Края ▸
Четверти ▸
Шестые ▸
Пять столбиков ▸
Шесть столбиков ▸
Семь столбиков ▸
Восемь столбиков ▸
"
        .to_string()
            + APP_COLUMNS_AND_OTHER
            + BOTTOM;
        assert_eq!(menu(Settings::default(), ONE_DISPLAY, 0), expected);
    }

    #[test]
    fn submenus_hold_their_categories() {
        let settings = Settings::default();
        let expected_moves = "\
Края ▸
  Налево
  Направо
  Вверх
  Вниз
";
        let items = layout(&settings);
        let text = render(&items, ONE_DISPLAY, &TERMINAL, APP, 1);
        assert!(text.contains(expected_moves), "{text}");
        assert!(text.contains(
            "Пять столбиков ▸\n  Первый столбик\n  Второй столбик\n  Третий столбик\n  \
             Четвёртый столбик\n  Пятый столбик\nШесть столбиков ▸\n"
        ));
        assert_eq!(
            actions(&submenu(settings, WindowActionCategory::Fourths)),
            vec![
                Action::FirstFourth,
                Action::SecondFourth,
                Action::ThirdFourth,
                Action::LastFourth,
                Action::FirstThreeFourths,
                Action::CenterThreeFourths,
                Action::LastThreeFourths,
            ]
        );
        assert_eq!(
            actions(&submenu(settings, WindowActionCategory::Sixths)).len(),
            6
        );
        assert_eq!(
            actions(&submenu(settings, WindowActionCategory::ColumnsEight)),
            Action::column_cases(8)
        );
        // Разделителей внутри подменю категорий нет.
        assert!(!submenu(settings, WindowActionCategory::Move).contains(&Item::Separator));
    }

    #[test]
    fn two_displays_show_display_items() {
        let text = menu(Settings::default(), TWO_DISPLAYS, 0);
        assert!(
            text.contains("Восстановить\n—\nСледующий экран\nПредыдущий экран\n—\nКрая ▸\n"),
            "{text}"
        );
        // Экраны считаются одним — пунктов снова нет.
        let combined = Displays {
            count: 2,
            combined: true,
        };
        let text = menu(Settings::default(), combined, 0);
        assert!(text.contains("Восстановить\n—\nКрая ▸\n"), "{text}");
    }

    #[test]
    fn additional_sizes_move_thirds_and_size_into_submenus() {
        let settings = Settings {
            show_additional_sizes: true,
            ..Settings::default()
        };
        let expected = "\
Левая половина
Правая половина
Центральная половина
Верхняя половина
Нижняя половина
—
Слева вверху
Справа вверху
Внизу слева
Внизу справа
—
Максимизировать
В центр
Восстановить
—
Размер ▸
Края ▸
Трети ▸
Четверти ▸
Шестые ▸
Восьмые ▸
Девятые ▸
Двенадцатые ▸
Шестнадцатые ▸
Пять столбиков ▸
Шесть столбиков ▸
Семь столбиков ▸
Восемь столбиков ▸
"
        .to_string()
            + APP_COLUMNS_AND_OTHER
            + BOTTOM;
        assert_eq!(menu(settings, ONE_DISPLAY, 0), expected);
        assert_eq!(
            actions(&submenu(settings, WindowActionCategory::Size)),
            vec![
                Action::AlmostMaximize,
                Action::MaximizeHeight,
                Action::Larger,
                Action::Smaller,
            ]
        );
        assert_eq!(
            actions(&submenu(settings, WindowActionCategory::Thirds)),
            vec![
                Action::FirstThird,
                Action::CenterThird,
                Action::LastThird,
                Action::FirstTwoThirds,
                Action::CenterTwoThirds,
                Action::LastTwoThirds,
            ]
        );
        assert_eq!(
            actions(&submenu(settings, WindowActionCategory::Sixteenths)).len(),
            16
        );
    }

    #[test]
    fn show_all_actions_is_one_flat_list() {
        let settings = Settings {
            show_all_actions: true,
            ..Settings::default()
        };
        let items = layout(&settings);
        let submenus: Vec<_> = items
            .iter()
            .filter_map(|item| match item {
                Item::Submenu(category, _) => Some(*category),
                _ => None,
            })
            .collect();
        assert_eq!(submenus, vec![WindowActionCategory::Other]);

        // Все действия с подписью оригинала — по порядку `WindowAction.active`.
        let flat = actions(&items);
        let with_titles: Vec<Action> = Action::active()
            .iter()
            .copied()
            .filter(|action| action.display_name().is_some())
            .collect();
        assert_eq!(flat, with_titles);

        let text = menu(settings, TWO_DISPLAYS, 0);
        assert!(text.starts_with("Левая половина\nПравая половина\n"));
        assert!(
            text.contains("—\nМаксимизировать\nПочти максимизировать\nМаксимизировать высоту\n")
        );
        assert!(text.contains("Предыдущий экран\n—\nНалево\nНаправо\nВверх\nВниз\n—\n"));
        assert!(text.contains("Нижняя шестая справа\n—\nВерхняя восьмая слева\n"));
        assert!(text.contains("Пятый столбик\n—\nПервый столбик\n"));
        assert!(text.ends_with(&("Восьмой столбик\n".to_string() + APP_COLUMNS_AND_OTHER + BOTTOM)));

        // С дополнительными размерами «Почти максимизировать» — отдельной группой.
        let settings = Settings {
            show_all_actions: true,
            show_additional_sizes: true,
            ..Settings::default()
        };
        assert!(menu(settings, ONE_DISPLAY, 0)
            .contains("—\nМаксимизировать\n—\nПочти максимизировать\n"));
    }

    #[test]
    fn todo_items_are_shown_only_when_enabled() {
        let settings = Settings {
            todo: true,
            ..Settings::default()
        };
        let text = menu(settings, ONE_DISPLAY, 0);
        assert!(
            text.contains(
                "Другое ▸\n—\nВключить режим Todo приложения\n\
                 Использовать Терминал в качестве приложения Todo\n\
                 Обновить положение Todo окна\n—\nИгнорировать Терминал\n"
            ),
            "{text}"
        );
        // «Использовать как окно Todo» — когда впереди Todo-приложение, но не его окно.
        let todo_app = FrontApp {
            is_todo_app: true,
            ..TERMINAL
        };
        let text = render(&layout(&settings), ONE_DISPLAY, &todo_app, APP, 0);
        assert!(text.contains("Todo\nИспользовать как окно Todo\nОбновить"));

        let other = actions(&submenu(settings, WindowActionCategory::Other));
        assert!(other.ends_with(&[Action::LeftTodo, Action::RightTodo]));
        let other = actions(&submenu(Settings::default(), WindowActionCategory::Other));
        assert!(!other.contains(&Action::LeftTodo) && !other.contains(&Action::RightTodo));
    }

    #[test]
    fn unknown_front_app_hides_its_items() {
        let text = render(
            &layout(&Settings::default()),
            ONE_DISPLAY,
            &FrontApp::default(),
            APP,
            0,
        );
        // Без имени приложения прячутся «Держать окна «…» столбиками» и «Игнорировать».
        assert!(
            text.contains("Окна приложения столбиками\n—\nДругое ▸\n—\nНастройки…\n"),
            "{text}"
        );
        assert!(!text.contains("Держать окна"));
        assert!(!text.contains("Игнорировать"));
    }

    #[test]
    fn app_columns_items_follow_the_column_layouts() {
        let text = menu(Settings::default(), ONE_DISPLAY, 0);
        assert!(
            text.contains(&("Восемь столбиков ▸\n".to_string() + APP_COLUMNS_AND_OTHER)),
            "{text}"
        );
        // Гаснут без активного окна, как действия; остальные служебные пункты — нет.
        for service in Service::ALL {
            let expected = matches!(service, Service::AppColumnsTile | Service::AppColumnsKeep);
            assert_eq!(service.needs_front_window(), expected, "{service:?}");
        }
    }

    #[test]
    fn other_submenu_lists_actions_without_original_menu_items() {
        let expected = "\
Другое ▸
  Верхняя треть
  Средняя треть
  Нижняя треть
  Верхние две трети
  Нижние две трети
  —
  Две трети в левом верхнем углу
  Две трети в правом верхнем углу
  Две трети в левом нижнем углу
  Две трети в правом нижнем углу
  —
  В центр повыше
  Заданный размер
  —
  Увеличить ширину
  Уменьшить ширину
  Увеличить высоту
  Уменьшить высоту
  —
  Удвоить ширину влево
  Удвоить ширину вправо
  Удвоить высоту вверх
  Удвоить высоту вниз
  —
  Левая половина окна
  Правая половина окна
  Верхняя половина окна
  Нижняя половина окна
  —
  Экран 1
  Экран 2
  —
  Все окна плиткой
  Все окна каскадом
  Окна приложения плиткой
  Окна приложения каскадом
  Все окна зеркально
";
        let other = vec![Item::Submenu(
            WindowActionCategory::Other,
            other_items(false),
        )];
        assert_eq!(render(&other, TWO_DISPLAYS, &TERMINAL, APP, 1), expected);

        // Экран один — пунктов экранов нет, и лишней черты тоже.
        let text = render(&other, ONE_DISPLAY, &TERMINAL, APP, 1);
        assert!(text.contains("  Нижняя половина окна\n  —\n  Все окна плиткой\n"));
        assert!(!text.contains("Экран"));

        // С Todo — все действия без пункта в оригинале, каждое ровно один раз.
        let mut listed = actions(&other_items(true));
        let mut expected: Vec<Action> = Action::active()
            .iter()
            .copied()
            .filter(|action| action.display_name().is_none())
            .collect();
        listed.sort();
        expected.sort();
        assert_eq!(listed, expected);
    }

    #[test]
    fn displays_follow_screen_count() {
        let three = Displays {
            count: 3,
            combined: false,
        };
        assert!(action_shown(Action::Display(3), three));
        assert!(!action_shown(Action::Display(4), three));
        assert!(action_shown(Action::NextDisplay, three));
        assert!(!action_shown(Action::Display(1), ONE_DISPLAY));
        assert!(!action_shown(Action::PreviousDisplay, ONE_DISPLAY));
        assert!(action_shown(Action::LeftHalf, ONE_DISPLAY));
    }

    #[test]
    fn separators_never_double_up() {
        // (разделитель, виден по своему правилу)
        let entries = [
            (true, true),   // в начале — нет
            (false, true),  // пункт
            (true, true),   // черта
            (false, false), // спрятанный пункт
            (true, true),   // вторая черта подряд — нет
            (false, true),  // пункт
            (true, true),   // в конце — нет
            (false, false),
        ];
        assert_eq!(
            collapse_separators(entries),
            vec![false, true, true, false, false, true, false, false]
        );
        assert_eq!(collapse_separators([]), Vec::<bool>::new());
    }

    #[test]
    fn settings_read_user_enabled_values() {
        let mut config = Config::default();
        assert_eq!(Settings::from_config(&config), Settings::default());
        config.show_additional_sizes_in_menu = Some(false);
        config.show_all_actions_in_menu = Some(true);
        config.todo = Some(true);
        assert_eq!(
            Settings::from_config(&config),
            Settings {
                show_all_actions: true,
                show_additional_sizes: false,
                todo: true,
            }
        );
    }
}
