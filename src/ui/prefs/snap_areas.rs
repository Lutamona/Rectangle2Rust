//! Вкладка «Области прилипания» (`SnapAreaViewController` оригинала,
//! docs/ui-spec.md §5).
//!
//! Вёрстка — стеки и ограничения сцены `HookshotConfigViewController` из
//! storyboard один в один: сверху флажки прилипания, под разделителем схема
//! горизонтального экрана — картинка «экрана» и восемь попапов вокруг (углы и
//! края слева и справа, верх и низ по центру), ниже — такая же схема
//! вертикального экрана, пока он подключён (`showHidePortrait`). Попап области:
//! «-», составные области этой области, разделитель и действия
//! (`snapping::area_model`); выбор сразу пишется в `landscapeSnapAreas` /
//! `portraitSnapAreas`.
//!
//! Как у оригинала, попап сначала содержит только «-» и выбранный пункт:
//! первая вёрстка 16 попапов со всеми пунктами (~75 в каждом) стоит около
//! 0,45 с. Оригинал добавлял остальные пункты следом, асинхронно; здесь — перед
//! первым открытием попапа (`menuNeedsUpdate:`).
//!
//! Контролы перечитывают настройки, когда их меняют не отсюда: «Сбросить
//! области прилипания» на вкладке «Основные», импорт, алерт о плитке macOS
//! («Выключить в …» снимает галку прилипания). Как у вкладки «Основные»,
//! содержимое лежит в прокрутке на случай низкого экрана.

mod logic;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBorderType, NSBox, NSBoxType, NSButton, NSFont, NSImage, NSImageFrameStyle, NSImageScaling,
    NSImageView, NSLayoutAttribute, NSLayoutConstraintOrientation, NSMenu, NSMenuDelegate,
    NSMenuItem, NSPopUpButton, NSScrollView, NSStackView, NSStackViewDistribution, NSTextAlignment,
    NSUserInterfaceLayoutOrientation, NSView, NSViewController,
};
use objc2_foundation::{NSCopying, NSPoint, NSRect, NSSize};

use super::general::VIEW_WIDTH;
use crate::config::{self, Config, Directional};
use crate::snapping::area_model::{self, DisplayOrientation};
use crate::ui::controls::{self, ns, DEFAULT_HIGH, DEFAULT_LOW, REQUIRED};
use crate::{events, log, mac_tiling, screens};
use logic::{PopupEntry, Toggle};

/// Ширина попапа области (`width = 190`).
const POPUP_WIDTH: f64 = 190.0;
/// Иконка действия в пункте попапа (`image.size = 18×12`).
const ITEM_IMAGE_SIZE: NSSize = NSSize::new(18.0, 12.0);
/// Отступы колонки сверху и снизу (`top + 20`, `bottom + 26`).
const TOP_INSET: f64 = 20.0;
const BOTTOM_INSET: f64 = 26.0;
/// `NSLineBreakByTruncatingTail`: длинная подпись попапа обрезается с конца.
const TRUNCATING_TAIL: usize = 4;

// ---------------------------------------------------------------- цель действий

define_class!(
    /// Цель действий флажков и попапов — то, чем в оригинале был
    /// `SnapAreaViewController`.
    #[unsafe(super(NSObject))]
    #[name = "R2PrefsSnapAreasTarget"]
    #[thread_kind = MainThreadOnly]
    struct Target;

    impl Target {
        #[unsafe(method(toggleWindowSnapping:))]
        fn toggle_window_snapping(&self, sender: &NSButton) {
            toggled(Toggle::WindowSnapping, sender);
        }

        #[unsafe(method(toggleUnsnapRestore:))]
        fn toggle_unsnap_restore(&self, sender: &NSButton) {
            toggled(Toggle::UnsnapRestore, sender);
        }

        #[unsafe(method(toggleMissionControlDragging:))]
        fn toggle_mission_control_dragging(&self, sender: &NSButton) {
            toggled(Toggle::MissionControlDragging, sender);
        }

        #[unsafe(method(toggleHapticFeedback:))]
        fn toggle_haptic_feedback(&self, sender: &NSButton) {
            toggled(Toggle::HapticFeedback, sender);
        }

        #[unsafe(method(toggleAnimateFootprint:))]
        fn toggle_animate_footprint(&self, sender: &NSButton) {
            toggled(Toggle::AnimateFootprint, sender);
        }

        #[unsafe(method(setLandscapeSnapArea:))]
        fn set_landscape_snap_area(&self, sender: &NSPopUpButton) {
            area_selected(DisplayOrientation::Landscape, sender);
        }

        #[unsafe(method(setPortraitSnapArea:))]
        fn set_portrait_snap_area(&self, sender: &NSPopUpButton) {
            area_selected(DisplayOrientation::Portrait, sender);
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

define_class!(
    /// Делегат меню попапов: перед открытием попапа — все его пункты.
    #[unsafe(super(NSObject))]
    #[name = "R2PrefsSnapAreaMenuDelegate"]
    #[thread_kind = MainThreadOnly]
    struct MenuDelegate;

    unsafe impl NSObjectProtocol for MenuDelegate {}

    unsafe impl NSMenuDelegate for MenuDelegate {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            let mtm = self.mtm();
            with_tab(|tab| {
                let popup = tab.popups.iter().find(|popup| {
                    popup
                        .button
                        .menu()
                        .is_some_and(|candidate| std::ptr::eq(&*candidate, menu))
                });
                if let Some(popup) = popup {
                    popup.fill(mtm);
                }
            });
        }
    }
);

impl MenuDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

/// Действие флажка — селектор оригинала.
fn toggle_action(toggle: Toggle) -> Sel {
    match toggle {
        Toggle::WindowSnapping => sel!(toggleWindowSnapping:),
        Toggle::UnsnapRestore => sel!(toggleUnsnapRestore:),
        Toggle::MissionControlDragging => sel!(toggleMissionControlDragging:),
        Toggle::HapticFeedback => sel!(toggleHapticFeedback:),
        Toggle::AnimateFootprint => sel!(toggleAnimateFootprint:),
    }
}

fn area_action(orientation: DisplayOrientation) -> Sel {
    match orientation {
        DisplayOrientation::Landscape => sel!(setLandscapeSnapArea:),
        DisplayOrientation::Portrait => sel!(setPortraitSnapArea:),
    }
}

/// Галку поставили или сняли. Менеджер прилипания перечитывает настройки сам.
fn toggled(toggle: Toggle, sender: &NSButton) {
    let on = controls::is_on(sender);
    config::update(|config| toggle.set(config, on));
    // Включили прилипание — проверить конфликт с плиткой macOS (всегда, а не
    // только в первый раз, как при запуске).
    if toggle == Toggle::WindowSnapping && on {
        mac_tiling::check_for_built_in_tiling(false);
    }
}

/// Выбор в попапе области — сразу в настройки.
fn area_selected(orientation: DisplayOrientation, sender: &NSPopUpButton) {
    let (popup_tag, item_tag) = (sender.tag(), sender.selectedTag());
    let Some((directional, choice)) = logic::popup_choice(popup_tag, item_tag) else {
        log!("Области прилипания: нет области {popup_tag} или пункта {item_tag}");
        return;
    };
    area_model::select(orientation, directional, choice);
}

// ---------------------------------------------------------------- вкладка

thread_local! {
    static TAB: RefCell<Option<Rc<SnapAreasTab>>> = const { RefCell::new(None) };
    static TARGET: RefCell<Option<Retained<Target>>> = const { RefCell::new(None) };
    /// Меню его не удерживают.
    static MENU_DELEGATE: RefCell<Option<Retained<MenuDelegate>>> = const { RefCell::new(None) };
    /// Иконки действий 18×12 для пунктов попапов, по одной на имя.
    static ITEM_IMAGES: RefCell<HashMap<&'static str, Retained<NSImage>>> =
        RefCell::new(HashMap::new());
}

fn with_tab(f: impl FnOnce(&SnapAreasTab)) {
    let tab = TAB.with(|slot| slot.borrow().clone());
    if let Some(tab) = tab {
        f(&tab);
    }
}

fn target(mtm: MainThreadMarker) -> Retained<Target> {
    TARGET.with(|slot| {
        slot.borrow_mut()
            .get_or_insert_with(|| Target::new(mtm))
            .clone()
    })
}

fn menu_delegate(mtm: MainThreadMarker) -> Retained<MenuDelegate> {
    MENU_DELEGATE.with(|slot| {
        slot.borrow_mut()
            .get_or_insert_with(|| MenuDelegate::new(mtm))
            .clone()
    })
}

/// Попап одной области схемы.
struct AreaPopup {
    orientation: DisplayOrientation,
    directional: Directional,
    button: Retained<NSPopUpButton>,
    /// В меню все пункты, а не только «-» и выбранный.
    filled: Cell<bool>,
}

impl AreaPopup {
    /// Показать выбранным пункт с тегом `tag`. Пока меню не заполнено — быстрый
    /// проход оригинала (`initialize(select:orientation:)`): «-» и этот пункт.
    fn show(&self, mtm: MainThreadMarker, tag: isize) {
        let Some(menu) = self.button.menu() else {
            return;
        };
        let shown = menu.numberOfItems() > 0 && self.button.selectedTag() == tag;
        if !self.filled.get() && !shown {
            menu.removeAllItems();
            for entry in logic::quick_entries(self.orientation, self.directional, tag) {
                menu.addItem(&menu_item(mtm, &entry));
            }
        }
        self.button.selectItemWithTag(tag);
    }

    /// Все пункты (`configure(select:orientation:)`). Меню вот-вот откроется
    /// на выбранном пункте, поэтому имеющиеся пункты остаются теми же
    /// объектами, а недостающие вставляются вокруг.
    fn fill(&self, mtm: MainThreadMarker) {
        if self.filled.replace(true) {
            return;
        }
        let Some(menu) = self.button.menu() else {
            return;
        };
        let entries = logic::popup_entries(self.orientation, self.directional);
        for (index, entry) in entries.iter().enumerate() {
            let index = index as isize;
            // `itemAtIndex:` за концом меню — исключение, а не nil.
            let present = match entry {
                PopupEntry::Item { tag, .. } => {
                    index < menu.numberOfItems()
                        && menu
                            .itemAtIndex(index)
                            .is_some_and(|item| !item.isSeparatorItem() && item.tag() == *tag)
                }
                PopupEntry::Separator => false,
            };
            if !present {
                menu.insertItem_atIndex(&menu_item(mtm, entry), index);
            }
        }
    }
}

struct SnapAreasTab {
    mtm: MainThreadMarker,
    root: Retained<NSScrollView>,
    document: Retained<NSView>,
    toggles: Vec<(Toggle, Retained<NSButton>)>,
    popups: Vec<AreaPopup>,
    /// Схема вертикального экрана (`portraitStackView`).
    portrait: Retained<NSStackView>,
}

/// Создать вкладку (один раз на процесс) и вернуть её контроллер.
pub(super) fn build(mtm: MainThreadMarker) -> Retained<NSViewController> {
    let tab = Rc::new(SnapAreasTab::new(mtm));
    let controller = NSViewController::new(mtm);
    controller.setView(&tab.root);
    TAB.with(|slot| *slot.borrow_mut() = Some(tab));
    // `configImported`, `defaultSnapAreas`, `windowSnapping` оригинала — любое
    // изменение этих настроек не из вкладки.
    config::subscribe(Box::new(config_changed));
    // `showHidePortrait` при смене экранов и активации приложения.
    events::on_screens_changed(update_portrait);
    events::on_app_will_become_active(update_portrait);
    controller
}

/// Высота, при которой вкладка видна целиком.
pub(super) fn content_height() -> f64 {
    let mut height = 0.0;
    with_tab(|tab| {
        tab.document.layoutSubtreeIfNeeded();
        height = tab.document.fittingSize().height;
    });
    height
}

fn config_changed(old: &Config, new: &Config) {
    if logic::affects_tab(old, new) {
        with_tab(|tab| tab.apply(new));
    }
}

fn update_portrait() {
    with_tab(SnapAreasTab::update_portrait);
}

impl SnapAreasTab {
    fn new(mtm: MainThreadMarker) -> SnapAreasTab {
        let target = target(mtm);
        let target: &AnyObject = &target;
        let config = config::current();

        // 1. Флажки: слева прилипание, возврат размера и Mission Control,
        // справа отклик и анимация; средняя колонка пустая (`5k8-dN-bzX`).
        let toggles: Vec<(Toggle, Retained<NSButton>)> = Toggle::ALL
            .iter()
            .map(|toggle| {
                let checkbox =
                    controls::checkbox(mtm, toggle.title(), target, toggle_action(*toggle));
                checkbox.setContentHuggingPriority_forOrientation(
                    DEFAULT_HIGH,
                    NSLayoutConstraintOrientation::Vertical,
                );
                (*toggle, checkbox)
            })
            .collect();
        let (left_views, right_views): (Vec<&NSView>, Vec<&NSView>) = {
            let views = |group: &[Toggle]| {
                toggles
                    .iter()
                    .filter(|(toggle, _)| group.contains(toggle))
                    .map(|(_, button)| button.as_ref())
                    .collect()
            };
            (views(&Toggle::LEFT), views(&Toggle::RIGHT))
        };
        let left = controls::column(mtm, 10.0, &left_views);
        left.setDetachesHiddenViews(true);
        let middle = controls::column(mtm, 8.0, &[]);
        let right = controls::column(mtm, 8.0, &right_views);
        for view in &right_views {
            view.heightAnchor()
                .constraintEqualToConstant(16.0)
                .setActive(true);
        }
        let toggles_row = controls::stack(
            mtm,
            NSUserInterfaceLayoutOrientation::Horizontal,
            NSLayoutAttribute::Top,
            20.0,
            &[&left, &middle, &right],
        );
        toggles_row.setDistribution(NSStackViewDistribution::FillEqually);
        toggles_row
            .bottomAnchor()
            .constraintEqualToAnchor(&middle.bottomAnchor())
            .setActive(true);

        // 3. Горизонтальный экран (`Vcp-1H-jc9`), 4. вертикальный (`3fZ-2P-Yw8`).
        let mut popups = Vec::new();
        let landscape = landscape_scheme(mtm, target, &config, &mut popups);
        let portrait = portrait_scheme(mtm, target, &config, &mut popups);

        // Колонка сверху вниз (`9T6-Lr-8m1`).
        let column = controls::stack(
            mtm,
            NSUserInterfaceLayoutOrientation::Vertical,
            NSLayoutAttribute::CenterX,
            20.0,
            &[&toggles_row, &separator(mtm), &landscape, &portrait],
        );
        column.setDetachesHiddenViews(true);
        column
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(100.0)
            .setActive(true);
        controls::activate(
            column.heightAnchor().constraintEqualToConstant(100.0),
            DEFAULT_HIGH,
        );
        column
            .widthAnchor()
            .constraintGreaterThanOrEqualToConstant(306.0)
            .setActive(true);
        controls::activate(
            column.widthAnchor().constraintEqualToConstant(306.0),
            DEFAULT_HIGH,
        );
        for constraint in [
            toggles_row
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&column.leadingAnchor(), 20.0),
            portrait
                .centerXAnchor()
                .constraintEqualToAnchor(&column.centerXAnchor()),
            portrait
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&column.leadingAnchor(), 40.0),
        ] {
            constraint.setActive(true);
        }

        // Прокрутка: документ прижат к верху, ширина — по ширине прокрутки.
        let document = controls::FlippedView::new(mtm);
        document.setTranslatesAutoresizingMaskIntoConstraints(false);
        document.addSubview(&column);
        for constraint in [
            column
                .topAnchor()
                .constraintEqualToAnchor_constant(&document.topAnchor(), TOP_INSET),
            document
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&column.bottomAnchor(), BOTTOM_INSET),
            column
                .centerXAnchor()
                .constraintEqualToAnchor(&document.centerXAnchor()),
        ] {
            constraint.setActive(true);
        }
        controls::activate(
            document.widthAnchor().constraintEqualToConstant(VIEW_WIDTH),
            DEFAULT_LOW,
        );

        let root = NSScrollView::new(mtm);
        root.setBorderType(NSBorderType::NoBorder);
        root.setDrawsBackground(false);
        root.setHasVerticalScroller(true);
        root.setAutohidesScrollers(true);
        root.setDocumentView(Some(&document));
        let clip = root.contentView();
        for constraint in [
            document
                .topAnchor()
                .constraintEqualToAnchor(&clip.topAnchor()),
            document
                .leadingAnchor()
                .constraintEqualToAnchor(&clip.leadingAnchor()),
            document
                .widthAnchor()
                .constraintEqualToAnchor(&clip.widthAnchor()),
        ] {
            constraint.setActive(true);
        }
        root.setFrameSize(NSSize::new(VIEW_WIDTH, 600.0));

        let tab = SnapAreasTab {
            mtm,
            root,
            document: Retained::into_super(document),
            toggles,
            popups,
            portrait,
        };
        // Флажок Mission Control и схема вертикального экрана — по условиям `viewDidLoad`.
        tab.toggle(Toggle::MissionControlDragging)
            .setHidden(!logic::shows_mission_control_dragging(&config, false));
        tab.portrait.setHidden(!logic::shows_scheme(
            DisplayOrientation::Portrait,
            portrait_display_connected(),
        ));
        tab.apply(&config);
        tab
    }

    fn toggle(&self, toggle: Toggle) -> &NSButton {
        let (_, button) = self
            .toggles
            .iter()
            .find(|(candidate, _)| *candidate == toggle)
            .expect("флажок есть");
        button
    }

    /// Показать настройки: галки, флажок Mission Control и выбор попапов.
    fn apply(&self, config: &Config) {
        for (toggle, button) in &self.toggles {
            controls::set_on(button, toggle.is_on(config));
        }
        let mission_control = self.toggle(Toggle::MissionControlDragging);
        let shown = !mission_control.isHidden();
        if logic::shows_mission_control_dragging(config, shown) != shown {
            mission_control.setHidden(false);
            self.relayout();
        }
        for popup in &self.popups {
            popup.show(
                self.mtm,
                logic::selected_tag(config, popup.orientation, popup.directional),
            );
        }
    }

    /// `showHidePortrait`: схема вертикального экрана — пока он подключён.
    fn update_portrait(&self) {
        let shown = logic::shows_scheme(DisplayOrientation::Portrait, portrait_display_connected());
        if self.portrait.isHidden() == !shown {
            return;
        }
        self.portrait.setHidden(!shown);
        self.relayout();
    }

    /// Пересчитать высоту и подогнать под неё окно.
    fn relayout(&self) {
        self.document.layoutSubtreeIfNeeded();
        if self.root.window().is_some() {
            super::fit_window(self.mtm, false);
        }
    }
}

fn portrait_display_connected() -> bool {
    area_model::portrait_display_connected(&screens::screens())
}

// ---------------------------------------------------------------- схемы экранов

/// Схема горизонтального экрана: три колонки попапов, по центру между верхним
/// и нижним — картинка экрана 200×125 (`Vcp-1H-jc9`).
fn landscape_scheme(
    mtm: MainThreadMarker,
    target: &AnyObject,
    config: &Config,
    popups: &mut Vec<AreaPopup>,
) -> Retained<NSStackView> {
    let orientation = DisplayOrientation::Landscape;
    let mut popup = |directional| {
        let popup = area_popup(mtm, target, config, orientation, directional);
        let button = popup.button.clone();
        popups.push(popup);
        button
    };
    let [top_left, left, bottom_left] = logic::LEFT_COLUMN.map(&mut popup);
    let [top, bottom] = logic::CENTER_COLUMN.map(&mut popup);
    let [top_right, right, bottom_right] = logic::RIGHT_COLUMN.map(&mut popup);
    // У верхнего попапа в storyboard вертикальное прилегание по умолчанию (250):
    // центральная колонка ниже боковых, и растягивается он.
    top.setContentHuggingPriority_forOrientation(
        DEFAULT_LOW,
        NSLayoutConstraintOrientation::Vertical,
    );
    for button in [&top, &bottom] {
        button.setContentCompressionResistancePriority_forOrientation(
            REQUIRED,
            NSLayoutConstraintOrientation::Vertical,
        );
    }
    let screen = screen_image(mtm, "wallpaperTiger", NSSize::new(200.0, 125.0));

    let vertical = |alignment, spacing, views: &[&NSView]| {
        controls::stack(
            mtm,
            NSUserInterfaceLayoutOrientation::Vertical,
            alignment,
            spacing,
            views,
        )
    };
    let left_column = vertical(
        NSLayoutAttribute::Leading,
        65.0,
        &[&top_left, &left, &bottom_left],
    );
    let center_column = vertical(NSLayoutAttribute::CenterX, 13.0, &[&top, &screen, &bottom]);
    let right_column = vertical(
        NSLayoutAttribute::Trailing,
        65.0,
        &[&top_right, &right, &bottom_right],
    );
    let scheme = controls::stack(
        mtm,
        NSUserInterfaceLayoutOrientation::Horizontal,
        NSLayoutAttribute::Top,
        14.0,
        &[&left_column, &center_column, &right_column],
    );
    for constraint in [
        right_column
            .topAnchor()
            .constraintEqualToAnchor(&left_column.topAnchor()),
        top.topAnchor()
            .constraintEqualToAnchor(&top_left.topAnchor()),
        right_column
            .bottomAnchor()
            .constraintEqualToAnchor(&left_column.bottomAnchor()),
        top_right
            .topAnchor()
            .constraintEqualToAnchor(&top_left.topAnchor()),
        center_column
            .topAnchor()
            .constraintEqualToAnchor(&left_column.topAnchor()),
        center_column
            .bottomAnchor()
            .constraintEqualToAnchor(&left_column.bottomAnchor()),
    ] {
        constraint.setActive(true);
    }
    scheme
}

/// Схема вертикального экрана под разделителем: колонки попапов с равными
/// промежутками, по центру картинка экрана 125×200 (`3fZ-2P-Yw8`).
fn portrait_scheme(
    mtm: MainThreadMarker,
    target: &AnyObject,
    config: &Config,
    popups: &mut Vec<AreaPopup>,
) -> Retained<NSStackView> {
    let orientation = DisplayOrientation::Portrait;
    let mut popup = |directional| {
        let popup = area_popup(mtm, target, config, orientation, directional);
        let button = popup.button.clone();
        popups.push(popup);
        button
    };
    let [top_left, left, bottom_left] = logic::LEFT_COLUMN.map(&mut popup);
    let [top, bottom] = logic::CENTER_COLUMN.map(&mut popup);
    let [top_right, right, bottom_right] = logic::RIGHT_COLUMN.map(&mut popup);
    for button in [&top, &bottom] {
        for orientation in [
            NSLayoutConstraintOrientation::Horizontal,
            NSLayoutConstraintOrientation::Vertical,
        ] {
            button.setContentCompressionResistancePriority_forOrientation(REQUIRED, orientation);
        }
    }
    let screen = screen_image(mtm, "wallpaperTigerVertical", NSSize::new(125.0, 200.0));

    let vertical = |alignment, spacing, views: &[&NSView]| {
        let stack = controls::stack(
            mtm,
            NSUserInterfaceLayoutOrientation::Vertical,
            alignment,
            spacing,
            views,
        );
        stack.setDistribution(NSStackViewDistribution::EqualCentering);
        stack
    };
    let left_column = vertical(
        NSLayoutAttribute::Leading,
        65.0,
        &[&top_left, &left, &bottom_left],
    );
    let center_column = vertical(NSLayoutAttribute::CenterX, 13.0, &[&top, &screen, &bottom]);
    let right_column = vertical(
        NSLayoutAttribute::CenterX,
        65.0,
        &[&top_right, &right, &bottom_right],
    );
    let columns = controls::row(mtm, 12.0, &[&left_column, &center_column, &right_column]);
    for constraint in [
        top_right
            .topAnchor()
            .constraintEqualToAnchor(&top_left.topAnchor()),
        top.topAnchor()
            .constraintEqualToAnchor(&top_left.topAnchor()),
    ] {
        constraint.setActive(true);
    }

    let line = separator(mtm);
    let scheme = controls::stack(
        mtm,
        NSUserInterfaceLayoutOrientation::Vertical,
        NSLayoutAttribute::CenterX,
        20.0,
        &[&line, &columns],
    );
    scheme.setDistribution(NSStackViewDistribution::EqualCentering);
    line.leadingAnchor()
        .constraintEqualToAnchor_constant(&scheme.leadingAnchor(), 65.0)
        .setActive(true);
    scheme
}

/// Разделитель (`NSBox` separator) без своей ширины — её даёт стек.
fn separator(mtm: MainThreadMarker) -> Retained<NSBox> {
    let line = NSBox::new(mtm);
    line.setBoxType(NSBoxType::Separator);
    line.setTranslatesAutoresizingMaskIntoConstraints(false);
    line.setContentHuggingPriority_forOrientation(
        DEFAULT_HIGH,
        NSLayoutConstraintOrientation::Vertical,
    );
    line
}

/// «Экран» схемы: картинка из Assets оригинала в серой рамке.
fn screen_image(mtm: MainThreadMarker, name: &str, size: NSSize) -> Retained<NSImageView> {
    let view = NSImageView::new(mtm);
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.setImageScaling(NSImageScaling::ScaleProportionallyDown);
    view.setImageFrameStyle(NSImageFrameStyle::GrayBezel);
    view.setImage(wallpaper(name).as_deref());
    view.setRefusesFirstResponder(true);
    for orientation in [
        NSLayoutConstraintOrientation::Horizontal,
        NSLayoutConstraintOrientation::Vertical,
    ] {
        view.setContentHuggingPriority_forOrientation(DEFAULT_LOW + 1.0, orientation);
    }
    controls::fix_width(&view, size.width);
    view.heightAnchor()
        .constraintEqualToConstant(size.height)
        .setActive(true);
    view
}

/// Цветная картинка из `packaging/icons` (не template).
fn wallpaper(name: &str) -> Option<Retained<NSImage>> {
    let image = controls::template_image(name)?.copy();
    image.setTemplate(false);
    Some(image)
}

// ---------------------------------------------------------------- попап области

/// Попап области (`width = 190`, шрифт `message`, длинные подписи обрезаются с
/// конца) с выбранным пунктом из настроек.
fn area_popup(
    mtm: MainThreadMarker,
    target: &AnyObject,
    config: &Config,
    orientation: DisplayOrientation,
    directional: Directional,
) -> AreaPopup {
    let button = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(POPUP_WIDTH, 24.0)),
        false,
    );
    button.setTranslatesAutoresizingMaskIntoConstraints(false);
    // Тег попапа — область (`Directional.rawValue`).
    button.setTag(directional.raw() as isize);
    // SAFETY: у цели есть метод действия с аргументом-отправителем, живёт она
    // до выхода (`TARGET`).
    unsafe {
        button.setTarget(Some(target));
        button.setAction(Some(area_action(orientation)));
    }
    button.setFont(Some(&NSFont::messageFontOfSize(13.0)));
    button.setAlignment(NSTextAlignment::Left);
    button.setImageScaling(NSImageScaling::ScaleProportionallyDown);
    if let Some(cell) = button.cell() {
        // SAFETY: `setLineBreakMode:` у ячейки принимает `NSLineBreakMode` (NSUInteger).
        let _: () = unsafe { msg_send![&*cell, setLineBreakMode: TRUNCATING_TAIL] };
    }
    button.setContentHuggingPriority_forOrientation(
        DEFAULT_HIGH,
        NSLayoutConstraintOrientation::Vertical,
    );
    controls::fix_width(&button, POPUP_WIDTH);
    if let Some(menu) = button.menu() {
        let delegate = menu_delegate(mtm);
        menu.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    }

    let popup = AreaPopup {
        orientation,
        directional,
        button,
        filled: Cell::new(false),
    };
    popup.show(mtm, logic::selected_tag(config, orientation, directional));
    popup
}

/// Пункт попапа: без действия (попап шлёт своё), у действий — иконка 18×12.
fn menu_item(mtm: MainThreadMarker, entry: &PopupEntry) -> Retained<NSMenuItem> {
    let PopupEntry::Item { title, tag, image } = entry else {
        return NSMenuItem::separatorItem(mtm);
    };
    // SAFETY: пункт без действия и без сочетания клавиш.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &ns(title),
            None,
            &ns(""),
        )
    };
    item.setTag(*tag);
    item.setImage(image.and_then(item_image).as_deref());
    item
}

/// Иконка действия 18×12 (копия template-картинки меню, одна на имя).
fn item_image(name: &'static str) -> Option<Retained<NSImage>> {
    if let Some(image) = ITEM_IMAGES.with(|cache| cache.borrow().get(name).cloned()) {
        return Some(image);
    }
    let image = controls::template_image(name)?.copy();
    image.setSize(ITEM_IMAGE_SIZE);
    ITEM_IMAGES.with(|cache| cache.borrow_mut().insert(name, image.clone()));
    Some(image)
}
