//! Где делить экран на половины — порт `ActiveSideSplitRatios.swift`.
//!
//! С включённым `cooperativeCornerResize` половины и углы делят экран не по настройкам
//! `horizontalSplitRatio`/`verticalSplitRatio`, а там, где на этом экране в последний раз
//! встала половина: действие «левая половина» с шириной ⅔ запоминает долю ⅔, и угол
//! «сверху слева» потом тоже займёт ⅔. Без этой настройки доли всегда из настроек.
//! Доли — `Float`, как в оригинале.
//!
//! В оригинале это общий объект `ActiveSideSplitRatios.shared`, с ним работает главный
//! поток. Здесь такой же объект заведён на поток (`with_shared`): приложение считает
//! раскладки на главном потоке, а тесты, которые идут в параллельных потоках, не мешают
//! друг другу. Настройки передаются параметром — там, где Swift читает `Defaults`.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::actions::Action;
use crate::config::{Config, CycleSize};
use crate::geometry::Rect;

/// Ключ экрана — его рабочая область с точностью до тысячной (`ScreenKey`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ScreenKey {
    min_x: i64,
    min_y: i64,
    width: i64,
    height: i64,
}

impl ScreenKey {
    fn new(frame: &Rect) -> Self {
        ScreenKey {
            min_x: Self::key_part(frame.min_x()),
            min_y: Self::key_part(frame.min_y()),
            width: Self::key_part(frame.w),
            height: Self::key_part(frame.h),
        }
    }

    /// `Int(round(value * 1000.0))`.
    fn key_part(value: f64) -> i64 {
        (value * 1000.0).round() as i64
    }
}

/// Запомненные доли одного экрана (`SplitRatios`): `None` — не запоминали.
#[derive(Clone, Copy, Debug, Default)]
struct SplitRatios {
    horizontal: Option<f32>,
    vertical: Option<f32>,
}

/// Доли сторон по экранам (`ActiveSideSplitRatios`).
#[derive(Debug)]
pub struct ActiveSideSplitRatios {
    ratios_by_screen: HashMap<ScreenKey, SplitRatios>,
    /// Проценты из настроек, с которыми запомнены доли: настройки поменялись —
    /// запомненное по этой оси сбрасывается.
    configured_horizontal_percent: f32,
    configured_vertical_percent: f32,
}

impl ActiveSideSplitRatios {
    /// Пустой набор под текущие настройки (`private init()`).
    pub fn new(config: &Config) -> Self {
        ActiveSideSplitRatios {
            ratios_by_screen: HashMap::new(),
            configured_horizontal_percent: config.horizontal_split_ratio,
            configured_vertical_percent: config.vertical_split_ratio,
        }
    }

    /// Доля левой половины экрана (`horizontalRatio(for:)`).
    pub fn horizontal_ratio(&mut self, screen_frame: &Rect, config: &Config) -> f32 {
        if config.cooperative_corner_resize {
            self.reset_changed_configured_defaults(config);
            return self
                .ratios_by_screen
                .get(&ScreenKey::new(screen_frame))
                .and_then(|ratios| ratios.horizontal)
                .unwrap_or_else(|| normalized(config.horizontal_split_ratio / 100.0));
        }
        config.horizontal_split_ratio / 100.0
    }

    /// Доля верхней половины экрана (`verticalRatio(for:)`).
    pub fn vertical_ratio(&mut self, screen_frame: &Rect, config: &Config) -> f32 {
        if config.cooperative_corner_resize {
            self.reset_changed_configured_defaults(config);
            return self
                .ratios_by_screen
                .get(&ScreenKey::new(screen_frame))
                .and_then(|ratios| ratios.vertical)
                .unwrap_or_else(|| normalized(config.vertical_split_ratio / 100.0));
        }
        config.vertical_split_ratio / 100.0
    }

    /// Запомнить долю после действия-половины (`recordSideAction`). `target_frame` —
    /// рамка из расчёта до гэпов, `screen_frame` — рабочая область экрана, куда встало окно.
    pub fn record_side_action(
        &mut self,
        action: Action,
        target_frame: &Rect,
        screen_frame: &Rect,
        config: &Config,
    ) {
        self.reset_changed_configured_defaults(config);

        if !(screen_frame.w > 0.0 && screen_frame.h > 0.0) {
            return;
        }

        match action {
            Action::LeftHalf => {
                self.set_horizontal_ratio((target_frame.w / screen_frame.w) as f32, screen_frame)
            }
            Action::RightHalf => self
                .set_horizontal_ratio(1.0 - (target_frame.w / screen_frame.w) as f32, screen_frame),
            Action::TopHalf => {
                self.set_vertical_ratio((target_frame.h / screen_frame.h) as f32, screen_frame)
            }
            Action::BottomHalf => self
                .set_vertical_ratio(1.0 - (target_frame.h / screen_frame.h) as f32, screen_frame),
            _ => {}
        }
    }

    /// Запомнить границу, которой на самом деле достигло окно при кооперативном ресайзе
    /// (`recordAchievedCooperativeAction`). Гэп делится между соседями поровну.
    pub fn record_achieved_cooperative_action(
        &mut self,
        action: Action,
        achieved_frame: &Rect,
        screen_frame: &Rect,
        gap_size: f64,
        config: &Config,
    ) {
        self.reset_changed_configured_defaults(config);

        if !(screen_frame.w > 0.0 && screen_frame.h > 0.0) {
            return;
        }

        let half_gap = swift_max_f64(0.0, gap_size) / 2.0;

        match action {
            Action::LeftHalf => self.record_leading_horizontal_boundary(
                achieved_frame.max_x() + half_gap,
                screen_frame,
            ),
            Action::RightHalf => self.record_leading_horizontal_boundary(
                achieved_frame.min_x() - half_gap,
                screen_frame,
            ),
            Action::TopHalf => self
                .record_leading_vertical_boundary(achieved_frame.min_y() - half_gap, screen_frame),
            Action::BottomHalf => self
                .record_leading_vertical_boundary(achieved_frame.max_y() + half_gap, screen_frame),
            Action::TopLeft => {
                self.record_leading_horizontal_boundary(
                    achieved_frame.max_x() + half_gap,
                    screen_frame,
                );
                self.record_leading_vertical_boundary(
                    achieved_frame.min_y() - half_gap,
                    screen_frame,
                );
            }
            Action::TopRight => {
                self.record_leading_horizontal_boundary(
                    achieved_frame.min_x() - half_gap,
                    screen_frame,
                );
                self.record_leading_vertical_boundary(
                    achieved_frame.min_y() - half_gap,
                    screen_frame,
                );
            }
            Action::BottomLeft => {
                self.record_leading_horizontal_boundary(
                    achieved_frame.max_x() + half_gap,
                    screen_frame,
                );
                self.record_leading_vertical_boundary(
                    achieved_frame.max_y() + half_gap,
                    screen_frame,
                );
            }
            Action::BottomRight => {
                self.record_leading_horizontal_boundary(
                    achieved_frame.min_x() - half_gap,
                    screen_frame,
                );
                self.record_leading_vertical_boundary(
                    achieved_frame.max_y() + half_gap,
                    screen_frame,
                );
            }
            _ => {}
        }
    }

    /// Забыть всё и взять проценты из настроек (`resetAll`).
    pub fn reset_all(&mut self, config: &Config) {
        self.ratios_by_screen.clear();
        self.configured_horizontal_percent = config.horizontal_split_ratio;
        self.configured_vertical_percent = config.vertical_split_ratio;
    }

    /// Забыть доли одного экрана (`reset(for:)`).
    pub fn reset(&mut self, screen_frame: &Rect) {
        self.ratios_by_screen.remove(&ScreenKey::new(screen_frame));
    }

    fn set_horizontal_ratio(&mut self, ratio: f32, screen_frame: &Rect) {
        self.ratios_by_screen
            .entry(ScreenKey::new(screen_frame))
            .or_default()
            .horizontal = Some(normalized(ratio));
    }

    fn set_vertical_ratio(&mut self, ratio: f32, screen_frame: &Rect) {
        self.ratios_by_screen
            .entry(ScreenKey::new(screen_frame))
            .or_default()
            .vertical = Some(normalized(ratio));
    }

    fn record_leading_horizontal_boundary(&mut self, boundary: f64, screen_frame: &Rect) {
        self.set_horizontal_ratio(
            ((boundary - screen_frame.min_x()) / screen_frame.w) as f32,
            screen_frame,
        );
    }

    fn record_leading_vertical_boundary(&mut self, boundary: f64, screen_frame: &Rect) {
        self.set_vertical_ratio(
            ((screen_frame.max_y() - boundary) / screen_frame.h) as f32,
            screen_frame,
        );
    }

    /// Проценты в настройках поменялись — запомненные доли по этой оси больше не в силе
    /// (`resetChangedConfiguredDefaults`).
    fn reset_changed_configured_defaults(&mut self, config: &Config) {
        let current_horizontal_percent = config.horizontal_split_ratio;
        let current_vertical_percent = config.vertical_split_ratio;

        if (current_horizontal_percent - self.configured_horizontal_percent).abs()
            > CycleSize::MATCHING_TOLERANCE
        {
            self.forget(|ratios| ratios.horizontal = None);
            self.configured_horizontal_percent = current_horizontal_percent;
        }

        if (current_vertical_percent - self.configured_vertical_percent).abs()
            > CycleSize::MATCHING_TOLERANCE
        {
            self.forget(|ratios| ratios.vertical = None);
            self.configured_vertical_percent = current_vertical_percent;
        }
    }

    /// Стереть долю по одной оси у всех экранов; пустые записи убрать
    /// (`resetHorizontalRatios`/`resetVerticalRatios` + `removeEmptyRatios`).
    fn forget(&mut self, clear: impl Fn(&mut SplitRatios)) {
        self.ratios_by_screen.retain(|_, ratios| {
            clear(ratios);
            ratios.horizontal.is_some() || ratios.vertical.is_some()
        });
    }
}

/// `min(1.0, max(0.0, ratio))` с семантикой Swift: `max(x, y)` — `y >= x ? y : x`,
/// `min(x, y)` — `y < x ? y : x` (NaN даёт 0).
fn normalized(ratio: f32) -> f32 {
    let lower = if ratio >= 0.0 { ratio } else { 0.0 };
    if lower < 1.0 {
        lower
    } else {
        1.0
    }
}

/// `Swift.max(x, y)` для CGFloat.
fn swift_max_f64(x: f64, y: f64) -> f64 {
    if y >= x {
        y
    } else {
        x
    }
}

thread_local! {
    static SHARED: RefCell<Option<ActiveSideSplitRatios>> = const { RefCell::new(None) };
}

/// Общий объект потока (`ActiveSideSplitRatios.shared`): создаётся при первом обращении
/// с настройками, действующими в этот момент, — как в оригинале.
pub fn with_shared<R>(config: &Config, f: impl FnOnce(&mut ActiveSideSplitRatios) -> R) -> R {
    SHARED.with(|shared| {
        let mut shared = shared.borrow_mut();
        let ratios = shared.get_or_insert_with(|| ActiveSideSplitRatios::new(config));
        f(ratios)
    })
}

/// `ActiveSideSplitRatios.shared.horizontalRatio(for:)`.
pub fn horizontal_ratio(screen_frame: &Rect, config: &Config) -> f32 {
    with_shared(config, |ratios| {
        ratios.horizontal_ratio(screen_frame, config)
    })
}

/// `ActiveSideSplitRatios.shared.verticalRatio(for:)`.
pub fn vertical_ratio(screen_frame: &Rect, config: &Config) -> f32 {
    with_shared(config, |ratios| ratios.vertical_ratio(screen_frame, config))
}

/// `ActiveSideSplitRatios.shared.recordSideAction(_:targetFrame:screenFrame:)` — зовёт
/// конвейер действия после гэпов, с рамкой из расчёта до гэпов.
pub fn record_side_action(
    action: Action,
    target_frame: &Rect,
    screen_frame: &Rect,
    config: &Config,
) {
    with_shared(config, |ratios| {
        ratios.record_side_action(action, target_frame, screen_frame, config)
    })
}

/// `ActiveSideSplitRatios.shared.recordAchievedCooperativeAction(...)` — зовёт конвейер
/// после согласованного ресайза с рамкой, которой окно достигло на самом деле. Рамку не
/// прочитать (`None`, у Swift — `CGRect.null`) — как в оригинале, только сверка долей с
/// настройками.
pub fn record_achieved_cooperative_action(
    action: Action,
    achieved_frame: Option<&Rect>,
    screen_frame: &Rect,
    gap_size: f64,
    config: &Config,
) {
    with_shared(config, |ratios| match achieved_frame {
        Some(frame) => {
            ratios.record_achieved_cooperative_action(action, frame, screen_frame, gap_size, config)
        }
        None => ratios.reset_changed_configured_defaults(config),
    })
}

/// `ActiveSideSplitRatios.shared.resetAll()`.
pub fn reset_all(config: &Config) {
    with_shared(config, |ratios| ratios.reset_all(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cooperative() -> Config {
        Config {
            cooperative_corner_resize: true,
            ..Config::default()
        }
    }

    #[test]
    fn without_cooperative_resize_ratios_come_from_settings() {
        let config = Config {
            horizontal_split_ratio: 60.0,
            ..Config::default()
        };
        let screen = Rect::new(0.0, 0.0, 1728.0, 1001.0);
        let mut ratios = ActiveSideSplitRatios::new(&config);
        ratios.record_side_action(
            Action::LeftHalf,
            &Rect::new(0.0, 0.0, 1152.0, 1001.0),
            &screen,
            &config,
        );
        assert_eq!(ratios.horizontal_ratio(&screen, &config), 60.0f32 / 100.0);
    }

    #[test]
    fn half_remembers_its_width_per_screen() {
        let config = cooperative();
        let screen = Rect::new(0.0, 0.0, 1728.0, 1001.0);
        let other = Rect::new(1728.0, 0.0, 2560.0, 1415.0);
        let mut ratios = ActiveSideSplitRatios::new(&config);

        ratios.record_side_action(
            Action::RightHalf,
            &Rect::new(576.0, 0.0, 1152.0, 1001.0),
            &screen,
            &config,
        );
        assert_eq!(
            ratios.horizontal_ratio(&screen, &config),
            1.0 - (1152.0f64 / 1728.0) as f32
        );
        assert_eq!(ratios.horizontal_ratio(&other, &config), 0.5);

        ratios.record_side_action(
            Action::TopHalf,
            &Rect::new(0.0, 334.0, 1728.0, 667.0),
            &screen,
            &config,
        );
        assert_eq!(
            ratios.vertical_ratio(&screen, &config),
            (667.0f64 / 1001.0) as f32
        );
    }

    #[test]
    fn changed_settings_reset_remembered_axis() {
        let mut config = cooperative();
        let screen = Rect::new(0.0, 0.0, 1728.0, 1001.0);
        let mut ratios = ActiveSideSplitRatios::new(&config);
        ratios.record_side_action(
            Action::LeftHalf,
            &Rect::new(0.0, 0.0, 576.0, 1001.0),
            &screen,
            &config,
        );
        ratios.record_side_action(
            Action::BottomHalf,
            &Rect::new(0.0, 0.0, 1728.0, 334.0),
            &screen,
            &config,
        );

        config.horizontal_split_ratio = 40.0;
        assert_eq!(ratios.horizontal_ratio(&screen, &config), 0.4);
        assert_eq!(
            ratios.vertical_ratio(&screen, &config),
            1.0 - (334.0f64 / 1001.0) as f32
        );
    }

    #[test]
    fn ratios_are_clamped_to_screen() {
        let config = cooperative();
        let screen = Rect::new(0.0, 0.0, 1000.0, 1000.0);
        let mut ratios = ActiveSideSplitRatios::new(&config);
        ratios.record_achieved_cooperative_action(
            Action::LeftHalf,
            &Rect::new(0.0, 0.0, 1200.0, 1000.0),
            &screen,
            10.0,
            &config,
        );
        assert_eq!(ratios.horizontal_ratio(&screen, &config), 1.0);
    }
}
