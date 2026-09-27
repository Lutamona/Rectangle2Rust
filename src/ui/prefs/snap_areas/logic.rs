//! Логика вкладки «Области прилипания» без AppKit: что показывают флажки и что
//! они пишут в настройки, какие контролы видны, какие пункты у попапа области и
//! какой из них выбран. Модель областей — `snapping::area_model`; вкладка
//! (`super`) только вызывает.

use crate::config::{Config, Directional};
use crate::snapping::area_model::{self, DisplayOrientation, SnapAreaChoice, SnapAreaMenuItem};

// ---------------------------------------------------------------- флажки

/// Флажок над схемами (`SnapAreaViewController`: пять галок).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    /// `windowSnapping`.
    WindowSnapping,
    /// `unsnapRestore`.
    UnsnapRestore,
    /// `missionControlDragging`: галка — «отключить», то есть настройка «нет».
    MissionControlDragging,
    /// `hapticFeedbackOnSnap`.
    HapticFeedback,
    /// `footprintAnimationDurationMultiplier`: 0,75 или 0.
    AnimateFootprint,
}

/// Множитель длительности анимации подсветки при галке «Анимировать след».
pub const ANIMATED_FOOTPRINT_MULTIPLIER: f32 = 0.75;

impl Toggle {
    /// Все флажки: левая колонка, затем правая.
    pub const ALL: [Toggle; 5] = [
        Toggle::WindowSnapping,
        Toggle::UnsnapRestore,
        Toggle::MissionControlDragging,
        Toggle::HapticFeedback,
        Toggle::AnimateFootprint,
    ];
    /// Левая колонка сверху вниз (storyboard `5Le-Om-VLZ`).
    pub const LEFT: [Toggle; 3] = [
        Toggle::WindowSnapping,
        Toggle::UnsnapRestore,
        Toggle::MissionControlDragging,
    ];
    /// Правая колонка (`GxY-ZJ-2qa`).
    pub const RIGHT: [Toggle; 2] = [Toggle::HapticFeedback, Toggle::AnimateFootprint];

    /// Подпись — русский перевод оригинала (`1ui-PL-TkR`, `UZP-5q-D5Y`,
    /// `3hZ-Cs-EZ6`, `r2Y-cY-tgn`, `kx2-tZ-lk8`).
    pub fn title(self) -> &'static str {
        match self {
            Toggle::WindowSnapping => "Защелкивание окон путем перетаскивания",
            Toggle::UnsnapRestore => "Восстановить размер окна после снятия привязки",
            Toggle::MissionControlDragging => "Отключить быстрое перетаскивание в Mission Control",
            Toggle::HapticFeedback => "Тактильный отклик",
            Toggle::AnimateFootprint => "Анимировать след",
        }
    }

    /// Галка стоит (`viewDidLoad`): прилипание и возврат размера — пока их не
    /// выключили явно, Mission Control — если функцию выключили явно,
    /// тактильный отклик — если включили явно, анимация — множитель больше 0.
    pub fn is_on(self, config: &Config) -> bool {
        match self {
            Toggle::WindowSnapping => config.window_snapping != Some(false),
            Toggle::UnsnapRestore => config.unsnap_restore != Some(false),
            Toggle::MissionControlDragging => config.mission_control_dragging == Some(false),
            Toggle::HapticFeedback => config.haptic_feedback_on_snap == Some(true),
            Toggle::AnimateFootprint => config.footprint_animation_duration_multiplier > 0.0,
        }
    }

    /// Записать галку в настройки (`toggle…:`).
    pub fn set(self, config: &mut Config, on: bool) {
        match self {
            Toggle::WindowSnapping => config.window_snapping = Some(on),
            Toggle::UnsnapRestore => config.unsnap_restore = Some(on),
            Toggle::MissionControlDragging => config.mission_control_dragging = Some(!on),
            Toggle::HapticFeedback => config.haptic_feedback_on_snap = Some(on),
            Toggle::AnimateFootprint => {
                config.footprint_animation_duration_multiplier = if on {
                    ANIMATED_FOOTPRINT_MULTIPLIER
                } else {
                    0.0
                }
            }
        }
    }
}

// ---------------------------------------------------------------- что видно

/// Флажок Mission Control — для тех, у кого функция уже выключена (скрытая
/// настройка): виден, если `missionControlDragging` = «нет»; показанный
/// остаётся, даже когда галку сняли (`shown` — виден сейчас).
pub fn shows_mission_control_dragging(config: &Config, shown: bool) -> bool {
    shown || config.mission_control_dragging == Some(false)
}

/// Схема экрана видна (`showHidePortrait`): горизонтального — всегда,
/// вертикального — пока подключён вертикальный экран.
pub fn shows_scheme(orientation: DisplayOrientation, portrait_display_connected: bool) -> bool {
    match orientation {
        DisplayOrientation::Landscape => true,
        DisplayOrientation::Portrait => portrait_display_connected,
    }
}

// ---------------------------------------------------------------- попапы

/// Попапы левой колонки схемы сверху вниз: угол, край, угол.
pub const LEFT_COLUMN: [Directional; 3] = [Directional::Tl, Directional::L, Directional::Bl];
/// Попапы по центру: над картинкой экрана и под ней.
pub const CENTER_COLUMN: [Directional; 2] = [Directional::T, Directional::B];
/// Попапы правой колонки.
pub const RIGHT_COLUMN: [Directional; 3] = [Directional::Tr, Directional::R, Directional::Br];

/// Пункт попапа области в том виде, в каком его ставит вкладка.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PopupEntry {
    Separator,
    Item {
        title: String,
        /// `NSMenuItem.tag`: −1 — выключено, ≤ −2 — составная область, иначе
        /// rawValue действия.
        tag: isize,
        /// Иконка действия (18×12); у «-» и составных областей нет.
        image: Option<&'static str>,
    },
}

/// Пункты попапа по порядку (`configure(select:orientation:)`).
pub fn popup_entries(orientation: DisplayOrientation, directional: Directional) -> Vec<PopupEntry> {
    area_model::menu_items(orientation, directional)
        .into_iter()
        .map(|item| match item {
            SnapAreaMenuItem::Separator => PopupEntry::Separator,
            SnapAreaMenuItem::Choice(choice) => PopupEntry::Item {
                title: choice.title(),
                tag: choice.tag() as isize,
                image: choice.image_name(),
            },
        })
        .collect()
}

/// Быстрый проход (`initialize(select:orientation:)`): в попапе только «-» и
/// выбранный пункт, если это не «-»; остальные пункты — перед открытием.
pub fn quick_entries(
    orientation: DisplayOrientation,
    directional: Directional,
    selected_tag: isize,
) -> Vec<PopupEntry> {
    let off = SnapAreaChoice::OFF_TAG as isize;
    popup_entries(orientation, directional)
        .into_iter()
        .filter(|entry| {
            matches!(entry, PopupEntry::Item { tag, .. } if *tag == off || *tag == selected_tag)
        })
        .collect()
}

/// Тег пункта, который попап показывает выбранным: что стоит в области, а
/// чего в попапе нет — «-» (`getSelectedTag`).
pub fn selected_tag(
    config: &Config,
    orientation: DisplayOrientation,
    directional: Directional,
) -> isize {
    area_model::selected_choice(config, orientation, directional).tag() as isize
}

/// Выбор в попапе (`setSnapArea(sender:type:)`): область — по тегу попапа
/// (`Directional.rawValue`, 1…8), что в неё поставить — по тегу пункта.
/// `None` — такого тега нет.
pub fn popup_choice(popup_tag: isize, item_tag: isize) -> Option<(Directional, SnapAreaChoice)> {
    let directional = Directional::from_raw(popup_tag as i64)
        .filter(|directional| Directional::CASES.contains(directional))?;
    let choice = SnapAreaChoice::from_tag(item_tag as i64)?;
    Some((directional, choice))
}

/// Поменялось то, что показывает вкладка: галки, видимость флажка Mission
/// Control или области какой-нибудь ориентации.
pub fn affects_tab(old: &Config, new: &Config) -> bool {
    Toggle::ALL
        .iter()
        .any(|toggle| toggle.is_on(old) != toggle.is_on(new))
        || DisplayOrientation::ALL.iter().any(|orientation| {
            area_model::snap_areas(old, *orientation) != area_model::snap_areas(new, *orientation)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Action;
    use crate::config::{CompoundSnapArea, SnapAreaConfig};

    use DisplayOrientation::{Landscape, Portrait};

    fn item_list(entries: &[PopupEntry]) -> Vec<(String, isize, Option<&'static str>)> {
        entries
            .iter()
            .filter_map(|entry| match entry {
                PopupEntry::Item { title, tag, image } => Some((title.clone(), *tag, *image)),
                PopupEntry::Separator => None,
            })
            .collect()
    }

    #[test]
    fn popup_of_an_area_lists_off_compounds_separator_and_actions() {
        // Нижний край горизонтального экрана: «-», пять составных, разделитель, 68 действий.
        let entries = popup_entries(Landscape, Directional::B);
        assert_eq!(entries.len(), 1 + 5 + 1 + 68);
        assert_eq!(entries[6], PopupEntry::Separator);
        let items = item_list(&entries);
        assert_eq!(items[0], ("-".to_string(), -1, None));
        assert_eq!(
            items[1],
            (
                "Трети, перетащите к центру на две трети".to_string(),
                -4,
                None
            )
        );
        let compound_tags: Vec<isize> = items[1..6].iter().map(|item| item.1).collect();
        assert_eq!(compound_tags, vec![-4, -6, -8, -9, -12]);
        // Первое действие — «Левая половина» с иконкой.
        assert_eq!(
            items[6],
            (
                "Левая половина".to_string(),
                Action::LeftHalf.raw() as isize,
                Some("leftHalfTemplate")
            )
        );
        assert!(items[6..]
            .iter()
            .all(|item| item.1 >= 0 && item.2.is_some()));

        // У угла составных нет: «-» и сразу разделитель.
        let corner = popup_entries(Portrait, Directional::Tr);
        assert_eq!(corner.len(), 2 + 68);
        assert_eq!(corner[1], PopupEntry::Separator);
        // Левый край вертикального экрана — три составные.
        let left = item_list(&popup_entries(Portrait, Directional::L));
        let compound_tags: Vec<isize> = left[1..4].iter().map(|item| item.1).collect();
        assert_eq!(compound_tags, vec![-2, -5, -10]);
        assert_eq!(left[3].0, "Верхняя или нижняя половина");
    }

    #[test]
    fn quick_pass_keeps_off_and_the_selected_item() {
        let config = Config::default();
        let selected = selected_tag(&config, Landscape, Directional::B);
        let quick = item_list(&quick_entries(Landscape, Directional::B, selected));
        assert_eq!(
            quick,
            vec![
                ("-".to_string(), -1, None),
                (
                    "Трети, перетащите к центру на две трети".to_string(),
                    -4,
                    None
                ),
            ]
        );
        let corner = item_list(&quick_entries(
            Landscape,
            Directional::Tl,
            Action::TopLeft.raw() as isize,
        ));
        assert_eq!(corner.len(), 2);
        assert_eq!(corner[1].2, Some("topLeftTemplate"));
        // Область выключена — только «-»; чужого тега в попапе нет — тоже.
        assert_eq!(quick_entries(Portrait, Directional::B, -1).len(), 1);
        assert_eq!(quick_entries(Portrait, Directional::B, -4).len(), 1);
    }

    #[test]
    fn choice_in_a_popup_is_written_and_read_back() {
        let mut config = Config::default();
        // По умолчанию: сверху — «Максимизировать», слева — составная.
        assert_eq!(
            selected_tag(&config, Landscape, Directional::T),
            Action::Maximize.raw() as isize
        );
        assert_eq!(selected_tag(&config, Landscape, Directional::L), -2);
        assert_eq!(selected_tag(&config, Portrait, Directional::B), -6);

        // Составная область у нижнего края.
        let (directional, choice) = popup_choice(Directional::B.raw() as isize, -9).expect("выбор");
        assert_eq!(directional, Directional::B);
        area_model::set_area_config(&mut config, Landscape, directional, choice.to_config());
        assert_eq!(
            config
                .landscape_snap_areas
                .as_ref()
                .and_then(|areas| areas.get(&Directional::B)),
            Some(&SnapAreaConfig::compound(CompoundSnapArea::Fourths))
        );
        assert_eq!(selected_tag(&config, Landscape, Directional::B), -9);
        // Вертикальная карта не тронута.
        assert_eq!(config.portrait_snap_areas, None);

        // Действие в верхний левый угол вертикального экрана.
        let (directional, choice) = popup_choice(
            Directional::Tl.raw() as isize,
            Action::Maximize.raw() as isize,
        )
        .expect("выбор");
        area_model::set_area_config(&mut config, Portrait, directional, choice.to_config());
        assert_eq!(
            selected_tag(&config, Portrait, Directional::Tl),
            Action::Maximize.raw() as isize
        );
        assert_eq!(
            selected_tag(&config, Landscape, Directional::Tl),
            Action::TopLeft.raw() as isize
        );

        // «-» выключает область.
        let (directional, choice) = popup_choice(Directional::R.raw() as isize, -1).expect("выбор");
        area_model::set_area_config(&mut config, Landscape, directional, choice.to_config());
        assert_eq!(
            area_model::area_config(&config, Landscape, Directional::R),
            None
        );
        assert_eq!(selected_tag(&config, Landscape, Directional::R), -1);

        // Неизвестные теги и центр — не выбор.
        assert_eq!(popup_choice(Directional::C.raw() as isize, -1), None);
        assert_eq!(popup_choice(0, -1), None);
        assert_eq!(popup_choice(Directional::T.raw() as isize, -100), None);
    }

    #[test]
    fn toggles_read_and_write_settings_like_the_original() {
        let mut config = Config::default();
        let on = |config: &Config| -> Vec<bool> {
            Toggle::ALL
                .iter()
                .map(|toggle| toggle.is_on(config))
                .collect()
        };
        // По умолчанию: прилипание и возврат размера включены, остальное — нет.
        assert_eq!(on(&config), vec![true, true, false, false, false]);

        Toggle::WindowSnapping.set(&mut config, false);
        assert_eq!(config.window_snapping, Some(false));
        Toggle::UnsnapRestore.set(&mut config, false);
        assert_eq!(config.unsnap_restore, Some(false));
        // Галка Mission Control — «отключить»: настройка становится «нет».
        Toggle::MissionControlDragging.set(&mut config, true);
        assert_eq!(config.mission_control_dragging, Some(false));
        Toggle::HapticFeedback.set(&mut config, true);
        assert_eq!(config.haptic_feedback_on_snap, Some(true));
        Toggle::AnimateFootprint.set(&mut config, true);
        assert_eq!(config.footprint_animation_duration_multiplier, 0.75);
        assert_eq!(on(&config), vec![false, false, true, true, true]);

        Toggle::MissionControlDragging.set(&mut config, false);
        assert_eq!(config.mission_control_dragging, Some(true));
        Toggle::AnimateFootprint.set(&mut config, false);
        assert_eq!(config.footprint_animation_duration_multiplier, 0.0);
        // Любой множитель больше нуля — галка стоит.
        config.footprint_animation_duration_multiplier = 0.3;
        assert!(Toggle::AnimateFootprint.is_on(&config));
    }

    #[test]
    fn hidden_controls_show_by_the_rules_of_the_original() {
        let mut config = Config::default();
        assert!(!shows_mission_control_dragging(&config, false));
        config.mission_control_dragging = Some(true);
        assert!(!shows_mission_control_dragging(&config, false));
        config.mission_control_dragging = Some(false);
        assert!(shows_mission_control_dragging(&config, false));
        // Сняли галку в этом окне — флажок не пропадает.
        config.mission_control_dragging = Some(true);
        assert!(shows_mission_control_dragging(&config, true));

        assert!(shows_scheme(Landscape, false));
        assert!(!shows_scheme(Portrait, false));
        assert!(shows_scheme(Portrait, true));
    }

    #[test]
    fn tab_refreshes_only_for_its_settings() {
        let old = Config::default();
        let mut new = old.clone();
        new.gap_size = 10.0;
        assert!(!affects_tab(&old, &new));
        // Явное значение, равное умолчанию, ничего не меняет на вкладке.
        new.window_snapping = Some(true);
        new.landscape_snap_areas = Some(area_model::default_snap_areas(Landscape));
        assert!(!affects_tab(&old, &new));

        let mut reset = old.clone();
        area_model::set_area_config(&mut reset, Portrait, Directional::B, None);
        assert!(affects_tab(&old, &reset));
        area_model::reset_areas(&mut reset);
        assert!(!affects_tab(&old, &reset));

        let mut alert = old.clone();
        alert.window_snapping = Some(false);
        assert!(affects_tab(&old, &alert));
        let mut hidden = old.clone();
        hidden.mission_control_dragging = Some(false);
        assert!(affects_tab(&old, &hidden));
    }
}
