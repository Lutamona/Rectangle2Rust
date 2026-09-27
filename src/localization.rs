//! Русские строки меню статус-бара (интерфейс только на русском, решение D2).
//!
//! Подписи действий, у которых в оригинале есть пункт меню, и заголовки
//! подменю живут рядом с действиями (`Action::display_name`,
//! `WindowActionCategory::display_name`). Здесь — подписи действий подменю
//! «Другое» (в оригинале пункта меню у них нет, решение D3), служебных пунктов
//! и алерта обновлений: из русского перевода оригинала (`Main.strings`,
//! `App.strings`), а чего там нет — переведено в том же стиле.

use crate::actions::Action;

/// Подпись действия в меню: как в оригинале, а у действий без пункта меню в
/// оригинале — подпись подменю «Другое». `None` — у несуществующих экранов и
/// столбиков.
pub fn action_title(action: Action) -> Option<&'static str> {
    action.display_name().or_else(|| other_title(action))
}

/// Подписи подменю «Другое»: в оригинале у этих действий пункта меню нет.
pub fn other_title(action: Action) -> Option<&'static str> {
    use Action::*;
    let title = match action {
        TopVerticalThird => "Верхняя треть",
        MiddleVerticalThird => "Средняя треть",
        BottomVerticalThird => "Нижняя треть",
        TopVerticalTwoThirds => "Верхние две трети",
        BottomVerticalTwoThirds => "Нижние две трети",
        // В оригинале «третями» названы углы в две трети ширины и половину высоты.
        TopLeftThird => "Две трети в левом верхнем углу",
        TopRightThird => "Две трети в правом верхнем углу",
        BottomLeftThird => "Две трети в левом нижнем углу",
        BottomRightThird => "Две трети в правом нижнем углу",
        CenterProminently => "В центр повыше",
        Specified => "Заданный размер",
        // «Увеличить/уменьшить ширину» — перевод оригинала (поповер Extras).
        LargerWidth => "Увеличить ширину",
        SmallerWidth => "Уменьшить ширину",
        LargerHeight => "Увеличить высоту",
        SmallerHeight => "Уменьшить высоту",
        DoubleWidthLeft => "Удвоить ширину влево",
        DoubleWidthRight => "Удвоить ширину вправо",
        DoubleHeightUp => "Удвоить высоту вверх",
        DoubleHeightDown => "Удвоить высоту вниз",
        // Окно сжимается вдвое к своему краю: остаётся его половина с этой стороны.
        HalveWidthLeft => "Левая половина окна",
        HalveWidthRight => "Правая половина окна",
        HalveHeightUp => "Верхняя половина окна",
        HalveHeightDown => "Нижняя половина окна",
        Display(number @ 1..=9) => DISPLAY_TITLES[number as usize - 1],
        TileAll => "Все окна плиткой",
        CascadeAll => "Все окна каскадом",
        TileActiveApp => "Окна приложения плиткой",
        CascadeActiveApp => "Окна приложения каскадом",
        ReverseAll => "Все окна зеркально",
        LeftTodo => "Todo слева",
        RightTodo => "Todo справа",
        _ => return None,
    };
    Some(title)
}

const DISPLAY_TITLES: [&str; 9] = [
    "Экран 1",
    "Экран 2",
    "Экран 3",
    "Экран 4",
    "Экран 5",
    "Экран 6",
    "Экран 7",
    "Экран 8",
    "Экран 9",
];

// ---------------------------------------------------------------- меню статус-бара

/// «Игнорировать frontmost.app» (`D99-0O-MB6.title`) с именем приложения.
pub fn ignore_app(app_name: &str) -> String {
    format!("Игнорировать {app_name}")
}

pub const SETTINGS: &str = "Настройки…";
pub const ABOUT: &str = "О программе";
pub const VIEW_LOGGING: &str = "Просмотр журнала…";
pub const CHECK_FOR_UPDATES: &str = "Проверить обновления…";

/// «Выход из Rectangle» (`A66-A4-cGD.title`) с именем приложения.
pub fn quit(app_name: &str) -> String {
    format!("Выход из {app_name}")
}

pub const TODO_MODE: &str = "Включить режим Todo приложения";

/// «Use frontmost.app as Todo App». Перевод оригинала потерял `frontmost.app`,
/// и имя приложения не подставлялось; здесь — по образцу окна «О Todo режиме»
/// («Использовать [Приложение] в качестве приложения Todo»).
pub fn todo_app(app_name: &str) -> String {
    format!("Использовать {app_name} в качестве приложения Todo")
}

/// «Use as Todo Window» — в оригинале без перевода.
pub const TODO_WINDOW: &str = "Использовать как окно Todo";
pub const TODO_REFLOW: &str = "Обновить положение Todo окна";

// ---------------------------------------------------------------- меню без доступа

pub const NOT_AUTHORIZED: &str = "Не авторизован для управления компьютером";
pub const AUTHORIZE: &str = "Авторизовать…";

/// «О Rectangle» (`jxe-nr-LDQ.title`) с именем приложения.
pub fn about_app(app_name: &str) -> String {
    format!("О {app_name}")
}

// ---------------------------------------------------------------- алерт обновлений

/// `UpdatesDisabledTitle` (таблица App).
pub const UPDATES_DISABLED_TITLE: &str = "Автообновление отключено";
/// `UpdatesDisabledText` (таблица App) — про эту сборку.
pub const UPDATES_DISABLED_TEXT: &str = "Это Rectangle 2, переписанная на Rust, — сборка из \
     исходников. Автообновления у неё нет: официальный релиз Rectangle заменил бы её обычной \
     версией. Обновить сборку можно командой ./build.sh --install в папке проекта.";
/// `OK` (таблица App).
pub const OK: &str = "ОК";

// ---------------------------------------------------------------- главное меню

/// Главное меню приложения (`Main Menu` storyboard, перевод оригинала). Оно
/// невидимо — приложение-агент без строки меню — и нужно ради сочетаний клавиш.
pub mod main_menu {
    pub const PREFERENCES: &str = "Настройки…";
    pub const SERVICES: &str = "Службы";
    pub const HIDE_OTHERS: &str = "Скрыть другое";
    pub const SHOW_ALL: &str = "Показать всё";
    pub const FILE: &str = "Файл";
    pub const NEW: &str = "Создать";
    pub const OPEN: &str = "Открыть…";
    pub const CLOSE: &str = "Закрыть";
    pub const SAVE: &str = "Сохранить…";
    pub const EDIT: &str = "Редактировать";
    pub const UNDO: &str = "Отменить";
    pub const REDO: &str = "Повторить";
    pub const CUT: &str = "Вырезать";
    pub const COPY: &str = "Копировать";
    pub const PASTE: &str = "Вставка";
    pub const PASTE_AND_MATCH_STYLE: &str = "Вставить и соотнести стили";
    pub const DELETE: &str = "Удалить";
    pub const SELECT_ALL: &str = "Выберите всё";
    pub const FIND: &str = "Найти";
    pub const FIND_ELLIPSIS: &str = "Найти…";
    pub const FIND_AND_REPLACE: &str = "Найти и заменить…";
    pub const FIND_NEXT: &str = "Найти дальше";
    pub const FIND_PREVIOUS: &str = "Найти предыдущий";
    pub const USE_SELECTION_FOR_FIND: &str = "Использовать выбранное для поиска";
    pub const JUMP_TO_SELECTION: &str = "Перейти к выбранному";
    pub const VIEW: &str = "Просмотр";
    pub const ENTER_FULL_SCREEN: &str = "Перейти в полноэкранный режим";
    pub const WINDOW: &str = "Окно";
    pub const MINIMIZE: &str = "Минимизировать";
    pub const HELP: &str = "Помощь";

    /// «Скрыть Rectangle» с именем приложения.
    pub fn hide(app_name: &str) -> String {
        format!("Скрыть {app_name}")
    }

    /// «Rectangle Справка» с именем приложения.
    pub fn help(app_name: &str) -> String {
        format!("{app_name} Справка")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_come_from_the_original_or_other_submenu() {
        assert_eq!(action_title(Action::LeftHalf), Some("Левая половина"));
        assert_eq!(action_title(Action::Restore), Some("Восстановить"));
        assert_eq!(
            action_title(Action::Column { count: 5, index: 1 }),
            Some("Первый столбик")
        );
        assert_eq!(action_title(Action::LargerWidth), Some("Увеличить ширину"));
        assert_eq!(action_title(Action::Display(3)), Some("Экран 3"));
        assert_eq!(action_title(Action::Display(10)), None);
        // У действий с пунктом в оригинале подписи «Другого» нет.
        assert_eq!(other_title(Action::LeftHalf), None);
        assert_eq!(other_title(Action::NextDisplay), None);
    }

    #[test]
    fn every_action_has_a_title() {
        for &action in Action::active() {
            let title = action_title(action);
            assert!(
                title.is_some_and(|title| !title.is_empty()),
                "{action:?} без подписи"
            );
            assert!(
                action.display_name().is_none() || other_title(action).is_none(),
                "{action:?}: две подписи"
            );
        }
    }

    #[test]
    fn app_name_is_substituted() {
        assert_eq!(ignore_app("Терминал"), "Игнорировать Терминал");
        assert_eq!(quit("Rectangle 2 (Rust)"), "Выход из Rectangle 2 (Rust)");
        assert_eq!(
            todo_app("Заметки"),
            "Использовать Заметки в качестве приложения Todo"
        );
        assert_eq!(about_app("Rectangle 2 (Rust)"), "О Rectangle 2 (Rust)");
    }
}
