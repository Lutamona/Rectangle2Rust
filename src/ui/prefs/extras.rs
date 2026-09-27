//! Поповер «Ещё» (`showExtraSettings` оригинала, docs/ui-spec.md §7) без строк
//! горячих клавиш: шаг ширины, три флажка и доли сторон.
//!
//! Создаётся при первом нажатии «⋯» и живёт до выхода, как в оригинале. В
//! оригинале колонки выровнены по полям шорткатов; их нет — колонка полей ввода
//! (160 pt) служит опорой: подписи слева равной ширины, флажки и попапы — по ней.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSButton, NSFont, NSLayoutConstraintOrientation, NSPopUpButton, NSPopover, NSPopoverBehavior,
    NSStackView, NSStackViewDistribution, NSTextAlignment, NSTextField, NSView, NSViewController,
};
use objc2_foundation::NSRectEdge;

use super::general::{self, Target};
use super::logic::{self, NumberSetting};
use crate::config::{self, Config, CycleSize};
use crate::ui::controls::{self, ns, REQUIRED};

/// Ширина колонки полей (в оригинале — ширина поля шортката).
const FIELD_WIDTH: f64 = 160.0;
/// tag пункта «Другое» (`HalfSplitRatioPopUpButton.otherTag`).
const OTHER_TAG: isize = -1;

thread_local! {
    static EXTRAS: RefCell<Option<Rc<Extras>>> = const { RefCell::new(None) };
}

struct Extras {
    popover: Retained<NSPopover>,
    width_step: Retained<NSTextField>,
    show_additional_sizes: Retained<NSButton>,
    overlap_offset: Retained<NSButton>,
    stack_badge: Retained<NSButton>,
    splits: [SplitRatio; 2],
}

/// Строка доли сторон: попап размеров («½ ⅔ ¾ ¼ ⅓ Другое») и поле процентов.
struct SplitRatio {
    setting: NumberSetting,
    popup: Retained<NSPopUpButton>,
    field: Retained<NSTextField>,
}

impl SplitRatio {
    /// `selectCurrentValue()`: размер, если процент совпадает с ним, и поле
    /// скрыто; иначе «Другое» и поле видно.
    fn select_current_value(&self, config: &Config) {
        match logic::half_split_preset(self.setting.get(config)) {
            Some(size) => {
                self.popup.selectItemWithTag(size.raw() as isize);
                self.field.setHidden(true);
            }
            None => {
                self.popup.selectItemWithTag(OTHER_TAG);
                self.field.setHidden(false);
            }
        }
    }
}

/// Показать поповер под кнопкой «⋯».
pub(super) fn show(target: &Target, sender: &NSButton) {
    let extras = EXTRAS.with(|slot| {
        slot.borrow_mut()
            .get_or_insert_with(|| Rc::new(Extras::new(target.mtm(), target)))
            .clone()
    });
    extras.popover.showRelativeToRect_ofView_preferredEdge(
        sender.bounds(),
        sender,
        NSRectEdge::MaxY,
    );
}

fn with_extras(f: impl FnOnce(&Extras)) {
    let extras = EXTRAS.with(|slot| slot.borrow().clone());
    if let Some(extras) = extras {
        f(&extras);
    }
}

/// Перечитать контролы из настроек (настройки поменяли снаружи).
pub(super) fn apply(config: &Config) {
    with_extras(|extras| extras.apply(config));
}

/// Какую настройку правит поле поповера.
pub(super) fn number_setting(field: &NSTextField) -> Option<NumberSetting> {
    let mut setting = None;
    with_extras(|extras| {
        if std::ptr::eq(&*extras.width_step, field) {
            setting = Some(NumberSetting::WidthStep);
        }
        for split in &extras.splits {
            if std::ptr::eq(&*split.field, field) {
                setting = Some(split.setting);
            }
        }
    });
    setting
}

/// Значение из поля сохранено (`defaultsSetAction`): попап доли сторон
/// выбирает пункт по новому значению.
pub(super) fn number_saved(setting: NumberSetting) {
    with_extras(|extras| {
        let config = config::current();
        for split in extras
            .splits
            .iter()
            .filter(|split| split.setting == setting)
        {
            split.select_current_value(&config);
        }
    });
}

/// `didSelectHalfSplitRatioPreset`: «Другое» открывает поле, размер
/// записывается процентом и прячет поле.
pub(super) fn split_preset_selected(sender: &NSPopUpButton) {
    with_extras(|extras| {
        let Some(split) = extras
            .splits
            .iter()
            .find(|split| std::ptr::eq(&*split.popup, sender))
        else {
            return;
        };
        let tag = sender.selectedTag();
        if tag == OTHER_TAG {
            split.field.setHidden(false);
            return;
        }
        let Some(size) = CycleSize::from_raw(tag as i64) else {
            crate::log!("Доля сторон: нет размера с tag {tag}");
            return;
        };
        let percent = size.percent_value();
        general::write(|config| split.setting.set(config, percent));
        split
            .field
            .setStringValue(&ns(&logic::integer_text(percent.round())));
        split.field.setHidden(true);
    });
}

/// Стеки поповера в оригинале созданы кодом (`NSStackView()`), у таких по
/// умолчанию gravity areas: лишнее место остаётся пустым, виды не растягиваются
/// (попап доли сторон 100 pt, даже когда поле процентов скрыто).
fn gravity_areas(stack: &NSStackView) {
    stack.setDistribution(NSStackViewDistribution::GravityAreas);
}

impl Extras {
    fn new(mtm: MainThreadMarker, target: &Target) -> Extras {
        let delegate = ProtocolObject::from_ref(target);
        let target: &AnyObject = target;
        let config = config::current();

        // Поле ввода с форматтером оригинала: выравнивание вправо, фокус
        // только по щелчку (`refusesFirstResponder`).
        let number_field = |setting: NumberSetting, width: f64| {
            let field = NSTextField::textFieldWithString(
                &ns(&setting.display(setting.get(&config), &general::separators())),
                mtm,
            );
            field.setAlignment(NSTextAlignment::Right);
            field.setRefusesFirstResponder(true);
            field.setTranslatesAutoresizingMaskIntoConstraints(false);
            controls::fix_width(&field, width);
            // SAFETY: цель живёт до выхода, делегат поля — слабая ссылка.
            unsafe { field.setDelegate(Some(delegate)) };
            field
        };
        let right_label = |text: &str| {
            let label = controls::label(mtm, text);
            label.setAlignment(NSTextAlignment::Right);
            label.setContentCompressionResistancePriority_forOrientation(
                REQUIRED,
                NSLayoutConstraintOrientation::Horizontal,
            );
            label
        };
        let checkbox = |title: &str, action, on: bool| {
            let checkbox = controls::checkbox(mtm, title, target, action);
            checkbox.setImageHugsTitle(true);
            controls::set_on(&checkbox, on);
            checkbox
        };

        // «Шаг ширины (px)».
        let width_step_label = right_label("Шаг ширины (px)");
        let width_step = number_field(NumberSetting::WidthStep, FIELD_WIDTH);
        let width_step_row = controls::row(mtm, 18.0, &[&width_step_label, &width_step]);
        gravity_areas(&width_step_row);

        let show_additional_sizes = checkbox(
            "Показывать дополнительные размеры в меню",
            sel!(toggleShowAdditionalSizesInMenu:),
            config.show_additional_sizes_in_menu == Some(true),
        );
        let overlap_offset = checkbox(
            "Смещать окно при наложении",
            sel!(toggleCyclingOverlapOffset:),
            config.cycling_overlap_offset == Some(true),
        );
        let stack_badge = checkbox(
            "Значок стопки окон при наведении",
            sel!(toggleStackBadge:),
            config.stack_badge == Some(true),
        );

        // «Доля сторон».
        let split_header = controls::label(mtm, "Доля сторон");
        split_header.setFont(Some(
            &NSFont::boldSystemFontOfSize(NSFont::systemFontSize()),
        ));
        split_header.setAlignment(NSTextAlignment::Center);
        split_header.setTranslatesAutoresizingMaskIntoConstraints(false);

        let mut split_rows = Vec::new();
        let mut split_labels = Vec::new();
        let mut split_controls = Vec::new();
        let splits = [
            (
                "По горизонтали (Л/П, %)",
                NumberSetting::HorizontalSplitRatio,
            ),
            ("По вертикали (В/Н, %)", NumberSetting::VerticalSplitRatio),
        ]
        .map(|(title, setting)| {
            let label = right_label(title);
            let mut items: Vec<(&str, isize)> = CycleSize::SORTED
                .iter()
                .map(|size| (size.title(), size.raw() as isize))
                .collect();
            items.push(("Другое", OTHER_TAG));
            let popup = controls::popup(mtm, &items, target, sel!(didSelectHalfSplitRatioPreset:));
            popup.setTranslatesAutoresizingMaskIntoConstraints(false);
            controls::fix_width(&popup, 100.0);
            let field = number_field(setting, 52.0);
            let field_controls = controls::row(mtm, 8.0, &[&popup, &field]);
            gravity_areas(&field_controls);
            controls::fix_width(&field_controls, FIELD_WIDTH);
            let row = controls::row(mtm, 18.0, &[&label, &field_controls]);
            gravity_areas(&row);
            split_rows.push(row);
            split_labels.push(label);
            split_controls.push(field_controls);
            let split = SplitRatio {
                setting,
                popup,
                field,
            };
            split.select_current_value(&config);
            split
        });

        let main = controls::column(mtm, 5.0, &[]);
        gravity_areas(&main);
        let arranged: [&NSView; 7] = [
            &width_step_row,
            &show_additional_sizes,
            &overlap_offset,
            &stack_badge,
            &split_header,
            &split_rows[0],
            &split_rows[1],
        ];
        for view in arranged {
            main.addArrangedSubview(view);
        }
        main.setCustomSpacing_afterView(10.0, &width_step_row);
        main.setCustomSpacing_afterView(8.0, &stack_badge);
        main.setCustomSpacing_afterView(10.0, &split_header);

        // Подписи равной ширины, флажки и попапы — по колонке полей.
        for label in &split_labels {
            label
                .widthAnchor()
                .constraintEqualToAnchor(&width_step_label.widthAnchor())
                .setActive(true);
        }
        for view in [&*show_additional_sizes, &*overlap_offset, &*stack_badge] {
            view.leadingAnchor()
                .constraintEqualToAnchor(&width_step.leadingAnchor())
                .setActive(true);
        }
        for view in &split_controls {
            view.trailingAnchor()
                .constraintEqualToAnchor(&width_step.trailingAnchor())
                .setActive(true);
        }
        split_header
            .widthAnchor()
            .constraintEqualToAnchor(&main.widthAnchor())
            .setActive(true);

        let container = NSView::new(mtm);
        container.addSubview(&main);
        for constraint in [
            main.topAnchor()
                .constraintEqualToAnchor_constant(&container.topAnchor(), 10.0),
            container
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&main.bottomAnchor(), 10.0),
            main.leadingAnchor()
                .constraintEqualToAnchor_constant(&container.leadingAnchor(), 15.0),
            container
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&main.trailingAnchor(), 15.0),
        ] {
            constraint.setActive(true);
        }

        let controller = NSViewController::new(mtm);
        controller.setView(&container);
        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setContentViewController(Some(&controller));

        Extras {
            popover,
            width_step,
            show_additional_sizes,
            overlap_offset,
            stack_badge,
            splits,
        }
    }

    fn apply(&self, config: &Config) {
        let separators = general::separators();
        if self.width_step.currentEditor().is_none() {
            self.width_step.setStringValue(&ns(
                &NumberSetting::WidthStep.display(config.width_step_size, &separators)
            ));
        }
        controls::set_on(
            &self.show_additional_sizes,
            config.show_additional_sizes_in_menu == Some(true),
        );
        controls::set_on(
            &self.overlap_offset,
            config.cycling_overlap_offset == Some(true),
        );
        controls::set_on(&self.stack_badge, config.stack_badge == Some(true));
        for split in &self.splits {
            if split.field.currentEditor().is_none() {
                split.field.setStringValue(&ns(&split
                    .setting
                    .display(split.setting.get(config), &separators)));
            }
            split.select_current_value(config);
        }
    }
}
