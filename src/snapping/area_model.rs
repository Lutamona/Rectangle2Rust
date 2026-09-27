//! Модель областей прилипания — порт `SnapAreaModel.swift` — и данные для
//! вкладки настроек «Области прилипания» (`SnapAreaViewController`).
//!
//! Что стоит в каждой из восьми областей горизонтального и вертикального
//! экрана, как это поменять и какие варианты предлагает попап области.
//! Функции над `Config` — чистые; `select`/`restore_defaults` меняют текущие
//! настройки через `config::update` (менеджер прилипания читает их на лету).
//!
//! Попап области (`SnapAreaViewController.configure`): «-» (выключить), затем
//! составные области, подходящие этой области и ориентации экрана,
//! разделитель и все действия, которые можно назначить на область
//! (`isDragSnappable` и есть подпись). Тег пункта — как у оригинала: −1 —
//! выключено, отрицательный — составная область, иначе rawValue действия.

use crate::actions::Action;
use crate::config::{
    self, default_landscape_snap_areas, default_portrait_snap_areas, CompoundSnapArea, Config,
    Directional, SnapAreaConfig, SnapAreaOption, SnapAreas,
};
use crate::geometry::Rect;
use crate::screens::Screen;

/// Ориентация экрана (`DisplayOrientation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DisplayOrientation {
    Landscape,
    Portrait,
}

impl DisplayOrientation {
    pub const ALL: [DisplayOrientation; 2] =
        [DisplayOrientation::Landscape, DisplayOrientation::Portrait];

    /// Ориентация экрана по его рамке (`frame.isLandscape`: ширина больше высоты).
    pub fn of(frame: &Rect) -> DisplayOrientation {
        if frame.is_landscape() {
            DisplayOrientation::Landscape
        } else {
            DisplayOrientation::Portrait
        }
    }
}

// ---------------------------------------------------------------- чтение и запись

/// Области для ориентации (`SnapAreaModel.landscape` / `portrait`): из
/// настроек, а если там ничего нет — по умолчанию.
pub fn snap_areas(config: &Config, orientation: DisplayOrientation) -> SnapAreas {
    match orientation {
        DisplayOrientation::Landscape => config.landscape_snap_areas_or_default(),
        DisplayOrientation::Portrait => config.portrait_snap_areas_or_default(),
    }
}

/// Области по умолчанию (`defaultLandscape` / `defaultPortrait`).
pub fn default_snap_areas(orientation: DisplayOrientation) -> SnapAreas {
    match orientation {
        DisplayOrientation::Landscape => default_landscape_snap_areas(),
        DisplayOrientation::Portrait => default_portrait_snap_areas(),
    }
}

/// Что стоит в области; `None` — область выключена.
pub fn area_config(
    config: &Config,
    orientation: DisplayOrientation,
    directional: Directional,
) -> Option<SnapAreaConfig> {
    snap_areas(config, orientation).get(&directional).copied()
}

/// Поставить в область действие или составную область, `None` — выключить
/// (`setConfig(type:directional:snapAreaConfig:)`): в настройки пишется вся
/// карта ориентации целиком.
pub fn set_area_config(
    config: &mut Config,
    orientation: DisplayOrientation,
    directional: Directional,
    area: Option<SnapAreaConfig>,
) {
    let mut areas = snap_areas(config, orientation);
    match area {
        Some(area) => {
            areas.insert(directional, area);
        }
        None => {
            areas.remove(&directional);
        }
    }
    match orientation {
        DisplayOrientation::Landscape => config.landscape_snap_areas = Some(areas),
        DisplayOrientation::Portrait => config.portrait_snap_areas = Some(areas),
    }
}

/// Вернуть области по умолчанию (часть «Сбросить настройки» оригинала:
/// `landscapeSnapAreas`/`portraitSnapAreas = nil`).
pub fn reset_areas(config: &mut Config) {
    config.landscape_snap_areas = None;
    config.portrait_snap_areas = None;
}

/// Есть ли вертикальный экран (`NSScreen.portraitDisplayConnected`): вкладка
/// показывает схему вертикального экрана только тогда.
pub fn portrait_display_connected(screens: &[Screen]) -> bool {
    screens.iter().any(|screen| !screen.frame.is_landscape())
}

/// У верхнего края настроена область (`isTopConfigured`); вертикальные
/// экраны учитываются, только если такой подключён. Нужна проверке
/// конфликта с плиткой macOS у верхнего края.
pub fn is_top_configured(config: &Config, portrait_display_connected: bool) -> bool {
    let configured = |area: Option<SnapAreaConfig>| {
        area.is_some_and(|area| area.action.is_some() || area.compound.is_some())
    };
    if configured(area_config(
        config,
        DisplayOrientation::Landscape,
        Directional::T,
    )) {
        return true;
    }
    portrait_display_connected
        && configured(area_config(
            config,
            DisplayOrientation::Portrait,
            Directional::T,
        ))
}

/// Перенос старых настроек в карты областей (`SnapAreaModel.migrate`):
/// `sixthsSnapArea` — шестые сверху и снизу; `ignoredSnapAreas` — выключенные
/// области, а выключенные обе короткие зоны стороны — просто половина.
/// Оригинал делает это при обновлении со сборки младше 64.
pub fn migrate(config: &mut Config) {
    use DisplayOrientation::{Landscape, Portrait};

    if config.sixths_snap_area == Some(true) {
        set_area_config(
            config,
            Landscape,
            Directional::T,
            Some(SnapAreaConfig::compound(CompoundSnapArea::TopSixths)),
        );
        set_area_config(
            config,
            Landscape,
            Directional::B,
            Some(SnapAreaConfig::compound(CompoundSnapArea::BottomSixths)),
        );
    }

    let ignored = config.ignored_snap_area_options();
    if ignored.0 <= 0 {
        return;
    }
    let options = [
        (Directional::Tl, SnapAreaOption::TOP_LEFT),
        (Directional::T, SnapAreaOption::TOP),
        (Directional::Tr, SnapAreaOption::TOP_RIGHT),
        (Directional::L, SnapAreaOption::LEFT),
        (Directional::R, SnapAreaOption::RIGHT),
        (Directional::Bl, SnapAreaOption::BOTTOM_LEFT),
        (Directional::B, SnapAreaOption::BOTTOM),
        (Directional::Br, SnapAreaOption::BOTTOM_RIGHT),
    ];
    for (directional, option) in options {
        if ignored.contains(option) {
            set_area_config(config, Landscape, directional, None);
            set_area_config(config, Portrait, directional, None);
        }
    }
    if ignored.contains(SnapAreaOption::BOTTOM_LEFT_SHORT)
        && ignored.contains(SnapAreaOption::TOP_LEFT_SHORT)
    {
        set_area_config(
            config,
            Landscape,
            Directional::L,
            Some(SnapAreaConfig::action(Action::LeftHalf.raw() as i64)),
        );
    }
    if ignored.contains(SnapAreaOption::BOTTOM_RIGHT_SHORT)
        && ignored.contains(SnapAreaOption::TOP_RIGHT_SHORT)
    {
        set_area_config(
            config,
            Landscape,
            Directional::R,
            Some(SnapAreaConfig::action(Action::RightHalf.raw() as i64)),
        );
    }
}

// ---------------------------------------------------------------- попап области

/// Выбор в попапе области.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapAreaChoice {
    /// «-»: область выключена.
    Off,
    Compound(CompoundSnapArea),
    Action(Action),
}

impl SnapAreaChoice {
    /// Тег «выключено».
    pub const OFF_TAG: i64 = -1;

    /// Тег пункта попапа (как `NSMenuItem.tag` оригинала).
    pub fn tag(self) -> i64 {
        match self {
            SnapAreaChoice::Off => Self::OFF_TAG,
            SnapAreaChoice::Compound(compound) => compound.raw(),
            SnapAreaChoice::Action(action) => action.raw() as i64,
        }
    }

    /// Выбор по тегу (`setSnapArea(sender:type:)`): `< −1` — составная
    /// область, `> −1` — действие, `−1` — выключено. `None` — тег неизвестен.
    pub fn from_tag(tag: i64) -> Option<SnapAreaChoice> {
        if tag == Self::OFF_TAG {
            return Some(SnapAreaChoice::Off);
        }
        if tag < Self::OFF_TAG {
            return CompoundSnapArea::from_raw(tag).map(SnapAreaChoice::Compound);
        }
        let raw = i32::try_from(tag).ok()?;
        Action::from_raw(raw).map(SnapAreaChoice::Action)
    }

    /// Подпись пункта.
    pub fn title(self) -> String {
        match self {
            SnapAreaChoice::Off => "-".to_string(),
            SnapAreaChoice::Compound(compound) => compound.display_name().to_string(),
            SnapAreaChoice::Action(action) => action
                .display_name()
                .map(str::to_string)
                .unwrap_or_else(|| action.name()),
        }
    }

    /// Иконка пункта (у действий — их картинка 18×12, у остальных нет).
    pub fn image_name(self) -> Option<&'static str> {
        match self {
            SnapAreaChoice::Action(action) => action.image_name(),
            _ => None,
        }
    }

    /// Что записать в настройки.
    pub fn to_config(self) -> Option<SnapAreaConfig> {
        match self {
            SnapAreaChoice::Off => None,
            SnapAreaChoice::Compound(compound) => Some(SnapAreaConfig::compound(compound)),
            SnapAreaChoice::Action(action) => Some(SnapAreaConfig::action(action.raw() as i64)),
        }
    }
}

/// Пункт попапа области.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapAreaMenuItem {
    Choice(SnapAreaChoice),
    Separator,
}

/// Составные области, которые предлагает попап области.
pub fn compound_choices(
    orientation: DisplayOrientation,
    directional: Directional,
) -> Vec<CompoundSnapArea> {
    CompoundSnapArea::ALL
        .iter()
        .copied()
        .filter(|compound| {
            compound.compatible_orientations().contains(&orientation)
                && compound.compatible_directionals().contains(&directional)
        })
        .collect()
}

/// Действия, которые можно назначить на область (одинаковы у всех областей).
pub fn action_choices() -> Vec<Action> {
    Action::active()
        .iter()
        .copied()
        .filter(|action| action.is_drag_snappable() && action.display_name().is_some())
        .collect()
}

/// Все пункты попапа по порядку (`configure(select:orientation:)`).
pub fn menu_items(
    orientation: DisplayOrientation,
    directional: Directional,
) -> Vec<SnapAreaMenuItem> {
    let mut items = vec![SnapAreaMenuItem::Choice(SnapAreaChoice::Off)];
    items.extend(
        compound_choices(orientation, directional)
            .into_iter()
            .map(|compound| SnapAreaMenuItem::Choice(SnapAreaChoice::Compound(compound))),
    );
    items.push(SnapAreaMenuItem::Separator);
    items.extend(
        action_choices()
            .into_iter()
            .map(|action| SnapAreaMenuItem::Choice(SnapAreaChoice::Action(action))),
    );
    items
}

/// Что показывает попап области: пункт с тегом из настроек (действие важнее
/// составной области, `getSelectedTag`); если такого пункта в попапе нет —
/// «-», как у оригинала.
pub fn selected_choice(
    config: &Config,
    orientation: DisplayOrientation,
    directional: Directional,
) -> SnapAreaChoice {
    let area = area_config(config, orientation, directional);
    let tag = area
        .and_then(|area| {
            area.action
                .or_else(|| area.compound.map(CompoundSnapArea::raw))
        })
        .unwrap_or(SnapAreaChoice::OFF_TAG);
    menu_items(orientation, directional)
        .into_iter()
        .find_map(|item| match item {
            SnapAreaMenuItem::Choice(choice) if choice.tag() == tag => Some(choice),
            _ => None,
        })
        .unwrap_or(SnapAreaChoice::Off)
}

/// Выбор в попапе (`setLandscapeSnapArea:` / `setPortraitSnapArea:`): сразу в
/// настройки.
pub fn select(orientation: DisplayOrientation, directional: Directional, choice: SnapAreaChoice) {
    config::update(|config| set_area_config(config, orientation, directional, choice.to_config()));
}

/// Вернуть области по умолчанию в текущих настройках.
pub fn restore_defaults() {
    config::update(reset_areas);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(frame: Rect) -> Screen {
        Screen {
            id: 1,
            frame,
            visible_frame: frame,
            name: String::new(),
            is_main: true,
            scale: 2.0,
            safe_area_top: 0.0,
        }
    }

    #[test]
    fn defaults_match_swift_model() {
        let config = Config::default();
        use DisplayOrientation::*;
        let action = |raw: i64| Some(SnapAreaConfig::action(raw));
        let compound = |area| Some(SnapAreaConfig::compound(area));
        assert_eq!(area_config(&config, Landscape, Directional::Tl), action(15));
        assert_eq!(area_config(&config, Landscape, Directional::T), action(2));
        assert_eq!(
            area_config(&config, Landscape, Directional::L),
            compound(CompoundSnapArea::LeftTopBottomHalf)
        );
        assert_eq!(
            area_config(&config, Landscape, Directional::B),
            compound(CompoundSnapArea::Thirds)
        );
        assert_eq!(
            area_config(&config, Portrait, Directional::R),
            compound(CompoundSnapArea::PortraitThirdsSide)
        );
        assert_eq!(
            area_config(&config, Portrait, Directional::B),
            compound(CompoundSnapArea::Halves)
        );
        assert_eq!(area_config(&config, Portrait, Directional::C), None);
    }

    #[test]
    fn setting_an_area_writes_the_whole_map() {
        let mut config = Config::default();
        set_area_config(
            &mut config,
            DisplayOrientation::Landscape,
            Directional::B,
            Some(SnapAreaConfig::compound(CompoundSnapArea::Fourths)),
        );
        let written = config.landscape_snap_areas.clone().expect("карта записана");
        assert_eq!(written.len(), 8);
        assert_eq!(
            written[&Directional::B],
            SnapAreaConfig::compound(CompoundSnapArea::Fourths)
        );
        assert_eq!(config.portrait_snap_areas, None);

        // Выключить — ключ пропадает.
        set_area_config(
            &mut config,
            DisplayOrientation::Landscape,
            Directional::Tl,
            None,
        );
        assert_eq!(
            area_config(&config, DisplayOrientation::Landscape, Directional::Tl),
            None
        );
        assert_eq!(
            config.landscape_snap_areas.as_ref().map(SnapAreas::len),
            Some(7)
        );

        reset_areas(&mut config);
        assert_eq!(config.landscape_snap_areas, None);
        assert_eq!(
            area_config(&config, DisplayOrientation::Landscape, Directional::Tl),
            Some(SnapAreaConfig::action(15))
        );
    }

    #[test]
    fn menu_lists_compounds_for_the_area_and_68_actions() {
        use DisplayOrientation::*;
        let compounds_of = |orientation, directional| -> Vec<i64> {
            compound_choices(orientation, directional)
                .into_iter()
                .map(CompoundSnapArea::raw)
                .collect()
        };
        // Таблица из docs/ui-spec.md §5.2.
        assert_eq!(
            compounds_of(Landscape, Directional::T),
            vec![-4, -6, -7, -9, -11]
        );
        assert_eq!(
            compounds_of(Landscape, Directional::B),
            vec![-4, -6, -8, -9, -12]
        );
        assert_eq!(compounds_of(Landscape, Directional::L), vec![-2]);
        assert_eq!(compounds_of(Landscape, Directional::R), vec![-3]);
        assert_eq!(compounds_of(Portrait, Directional::T), vec![-6]);
        assert_eq!(compounds_of(Portrait, Directional::L), vec![-2, -5, -10]);
        assert_eq!(compounds_of(Portrait, Directional::R), vec![-3, -5, -10]);
        for corner in [
            Directional::Tl,
            Directional::Tr,
            Directional::Bl,
            Directional::Br,
        ] {
            assert!(compounds_of(Landscape, corner).is_empty());
            assert!(compounds_of(Portrait, corner).is_empty());
        }

        let actions = action_choices();
        assert_eq!(actions.len(), 68);
        let tags: Vec<i64> = actions.iter().map(|action| action.raw() as i64).collect();
        // Начало списка — как в попапе оригинала: Left, Right, Center, Top, Bottom, углы.
        assert_eq!(&tags[..9], &[0, 1, 30, 11, 10, 15, 16, 13, 14]);
        assert!(!actions.contains(&Action::TopLeftNinth));
        assert!(!actions.contains(&Action::Restore));

        let items = menu_items(Landscape, Directional::Tl);
        assert_eq!(items[0], SnapAreaMenuItem::Choice(SnapAreaChoice::Off));
        // У угла составных нет, но разделитель есть.
        assert_eq!(items[1], SnapAreaMenuItem::Separator);
        assert_eq!(items.len(), 2 + 68);
        let items = menu_items(Landscape, Directional::B);
        assert_eq!(items.len(), 1 + 5 + 1 + 68);
        assert_eq!(items[6], SnapAreaMenuItem::Separator);
    }

    #[test]
    fn choices_round_trip_through_tags_and_have_russian_titles() {
        for item in menu_items(DisplayOrientation::Landscape, Directional::B) {
            let SnapAreaMenuItem::Choice(choice) = item else {
                continue;
            };
            assert_eq!(SnapAreaChoice::from_tag(choice.tag()), Some(choice));
            assert!(!choice.title().is_empty());
        }
        assert_eq!(SnapAreaChoice::Off.title(), "-");
        assert_eq!(
            SnapAreaChoice::Compound(CompoundSnapArea::Thirds).title(),
            "Трети, перетащите к центру на две трети"
        );
        assert_eq!(
            SnapAreaChoice::Action(Action::LeftHalf).title(),
            "Левая половина"
        );
        assert_eq!(
            SnapAreaChoice::Action(Action::LeftHalf).image_name(),
            Some("leftHalfTemplate")
        );
        assert_eq!(SnapAreaChoice::from_tag(-100), None);
        assert_eq!(SnapAreaChoice::from_tag(7), None);
        assert_eq!(SnapAreaChoice::Off.to_config(), None);
        assert_eq!(
            SnapAreaChoice::Action(Action::Maximize).to_config(),
            Some(SnapAreaConfig::action(2))
        );
    }

    #[test]
    fn selected_choice_follows_the_popup_rules() {
        use DisplayOrientation::*;
        let mut config = Config::default();
        assert_eq!(
            selected_choice(&config, Landscape, Directional::L),
            SnapAreaChoice::Compound(CompoundSnapArea::LeftTopBottomHalf)
        );
        assert_eq!(
            selected_choice(&config, Landscape, Directional::T),
            SnapAreaChoice::Action(Action::Maximize)
        );
        // Действие важнее составной области.
        set_area_config(
            &mut config,
            Landscape,
            Directional::B,
            Some(SnapAreaConfig {
                compound: Some(CompoundSnapArea::Thirds),
                action: Some(Action::BottomHalf.raw() as i64),
            }),
        );
        assert_eq!(
            selected_choice(&config, Landscape, Directional::B),
            SnapAreaChoice::Action(Action::BottomHalf)
        );
        // Действия нет в попапе (девятая) — «-».
        set_area_config(
            &mut config,
            Landscape,
            Directional::Tr,
            Some(SnapAreaConfig::action(Action::TopRightNinth.raw() as i64)),
        );
        assert_eq!(
            selected_choice(&config, Landscape, Directional::Tr),
            SnapAreaChoice::Off
        );
        // Составная область не для этого края — «-».
        set_area_config(
            &mut config,
            Landscape,
            Directional::L,
            Some(SnapAreaConfig::compound(CompoundSnapArea::Thirds)),
        );
        assert_eq!(
            selected_choice(&config, Landscape, Directional::L),
            SnapAreaChoice::Off
        );
    }

    #[test]
    fn top_configured_counts_portrait_only_when_connected() {
        let mut config = Config::default();
        assert!(is_top_configured(&config, false));
        set_area_config(
            &mut config,
            DisplayOrientation::Landscape,
            Directional::T,
            None,
        );
        assert!(!is_top_configured(&config, false));
        assert!(is_top_configured(&config, true));
        set_area_config(
            &mut config,
            DisplayOrientation::Portrait,
            Directional::T,
            None,
        );
        assert!(!is_top_configured(&config, true));
        // Пустая запись (`{}`) — не настроено.
        set_area_config(
            &mut config,
            DisplayOrientation::Landscape,
            Directional::T,
            Some(SnapAreaConfig::default()),
        );
        assert!(!is_top_configured(&config, true));

        let landscape = screen(Rect::new(0.0, 0.0, 1512.0, 982.0));
        let portrait = screen(Rect::new(1512.0, 0.0, 1080.0, 1920.0));
        assert!(!portrait_display_connected(std::slice::from_ref(
            &landscape
        )));
        assert!(portrait_display_connected(&[landscape, portrait]));
    }

    #[test]
    fn migration_moves_old_flags_into_maps() {
        use DisplayOrientation::*;
        let mut config = Config {
            sixths_snap_area: Some(true),
            ignored_snap_areas: (SnapAreaOption::TOP_LEFT
                | SnapAreaOption::BOTTOM_LEFT_SHORT
                | SnapAreaOption::TOP_LEFT_SHORT)
                .0,
            ..Config::default()
        };
        migrate(&mut config);
        assert_eq!(
            area_config(&config, Landscape, Directional::T),
            Some(SnapAreaConfig::compound(CompoundSnapArea::TopSixths))
        );
        assert_eq!(
            area_config(&config, Landscape, Directional::B),
            Some(SnapAreaConfig::compound(CompoundSnapArea::BottomSixths))
        );
        assert_eq!(area_config(&config, Landscape, Directional::Tl), None);
        assert_eq!(area_config(&config, Portrait, Directional::Tl), None);
        assert_eq!(
            area_config(&config, Landscape, Directional::L),
            Some(SnapAreaConfig::action(0))
        );
        // Правую сторону не трогали.
        assert_eq!(
            area_config(&config, Landscape, Directional::R),
            Some(SnapAreaConfig::compound(
                CompoundSnapArea::RightTopBottomHalf
            ))
        );

        // Без старых флагов — ничего не меняется.
        let mut untouched = Config::default();
        migrate(&mut untouched);
        assert_eq!(untouched, Config::default());
    }
}
