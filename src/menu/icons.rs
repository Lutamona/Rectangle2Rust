//! Картинки меню: иконка статус-бара, иконки действий, системные символы.
//!
//! PNG — те же, что в Assets оригинала (`packaging/icons`, в бандле
//! `Resources/icons`). Файл `<имя>@2x.png`, если он есть, добавляется вторым
//! представлением — на Retina иконка чёткая. Иконка пункта — 18×12 pt, как
//! `menuItem.image?.size` оригинала; трети на портретном экране — копия,
//! повёрнутая на 270° (`NSImage.rotated(by:)`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::ptr;

use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_app_kit::{
    NSAffineTransformNSAppKitAdditions, NSBitmapImageRep, NSCompositingOperation,
    NSDeviceRGBColorSpace, NSGraphicsContext, NSImage, NSImageRep,
};
use objc2_foundation::{NSAffineTransform, NSBundle, NSPoint, NSRect, NSSize, NSString};

use crate::actions::{Action, WindowActionCategory};

/// Размер иконки пункта меню (`NSSize(width: 18, height: 12)` оригинала).
const ITEM_ICON_SIZE: NSSize = NSSize::new(18.0, 12.0);

/// Масштабы, в которых перерисовываются картинки: обычный экран и Retina.
const SCALES: [f64; 2] = [1.0, 2.0];

/// Иконки пунктов: (имя картинки, повёрнута ли) → картинка.
type IconCache = HashMap<(Option<&'static str>, bool), Retained<NSImage>>;

thread_local! {
    static ITEM_ICONS: RefCell<IconCache> = RefCell::new(HashMap::new());
}

/// Иконка статус-бара (`StatusTemplate`, 22×22 pt).
pub fn status_icon() -> Option<Retained<NSImage>> {
    load("StatusTemplate")
}

/// Иконка пункта действия, 18×12 pt; `portrait` — главный экран портретный,
/// и тогда у третей повёрнутая копия. У действий без картинки — пустая
/// картинка того же размера (`NSImage()` в оригинале): подписи пунктов
/// стоят ровно.
pub fn action_icon(action: Action, portrait: bool) -> Retained<NSImage> {
    let name = action.image_name();
    let rotate = portrait && action.classification() == Some(WindowActionCategory::Thirds);
    ITEM_ICONS.with(|cache| {
        cache
            .borrow_mut()
            .entry((name, rotate))
            .or_insert_with(|| item_icon(name, rotate))
            .clone()
    })
}

/// Системный символ (SF Symbols) — иконки «Настройки…», «Просмотр журнала…»,
/// «Проверить обновления…» (`addMenuIcons`). Меню macOS 27 не рисует
/// символ в пункте (место под картинку остаётся пустым), поэтому он
/// перерисовывается в обычные битмапы.
pub fn symbol(name: &str) -> Option<Retained<NSImage>> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(name),
        None,
    )?;
    Some(redrawn(&image, 0.0))
}

fn item_icon(name: Option<&str>, rotate: bool) -> Retained<NSImage> {
    let Some(image) = name.and_then(load) else {
        return NSImage::initWithSize(NSImage::alloc(), ITEM_ICON_SIZE);
    };
    image.setSize(ITEM_ICON_SIZE);
    if rotate {
        redrawn(&image, 270.0)
    } else {
        image
    }
}

/// PNG вместе с `@2x`, как template: меню само красит её под тему.
fn load(name: &str) -> Option<Retained<NSImage>> {
    let path = icon_path(&format!("{name}.png"))?;
    let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(&path))?;
    if let Some(path) = icon_path(&format!("{name}@2x.png")) {
        if let Some(rep) = NSImageRep::imageRepWithContentsOfFile(&NSString::from_str(&path)) {
            // Размер в пунктах тот же, пикселей вдвое больше.
            rep.setSize(image.size());
            image.addRepresentation(&rep);
        }
    }
    image.setTemplate(true);
    Some(image)
}

/// Файл из `Resources/icons` бандла, а без бандла (`cargo run`, тесты) — из
/// `packaging/icons` исходников.
fn icon_path(file: &str) -> Option<String> {
    if let Some(resources) = NSBundle::mainBundle().resourcePath() {
        let path = format!("{resources}/icons/{file}");
        if Path::new(&path).exists() {
            return Some(path);
        }
    }
    let path = format!("{}/packaging/icons/{file}", env!("CARGO_MANIFEST_DIR"));
    Path::new(&path).exists().then_some(path)
}

/// Картинка, перерисованная в битмапы 1x и 2x с поворотом на `degrees`
/// вокруг центра — `NSImage.rotated(by:)` оригинала: размер результата —
/// описанный прямоугольник. Template, как и исходная.
fn redrawn(image: &NSImage, degrees: f64) -> Retained<NSImage> {
    let size = image.size();
    let radians = degrees.to_radians();
    let (sin, cos) = (radians.sin().abs(), radians.cos().abs());
    let new_size = NSSize::new(
        size.height * sin + size.width * cos,
        size.width * sin + size.height * cos,
    );
    let bounds = NSRect::new(
        NSPoint::new(
            (new_size.width - size.width) / 2.0,
            (new_size.height - size.height) / 2.0,
        ),
        size,
    );

    let result = NSImage::initWithSize(NSImage::alloc(), new_size);
    for scale in SCALES {
        if let Some(rep) = draw(image, new_size, bounds, degrees, scale) {
            result.addRepresentation(&rep);
        }
    }
    result.setTemplate(true);
    result
}

/// Одно представление для `redrawn`: битмап `new_size × scale` пикселей.
fn draw(
    image: &NSImage,
    new_size: NSSize,
    bounds: NSRect,
    degrees: f64,
    scale: f64,
) -> Option<Retained<NSImageRep>> {
    let width = (new_size.width * scale).round() as isize;
    let height = (new_size.height * scale).round() as isize;
    // SAFETY: буфер выделяет сам битмап (planes = NULL), параметры — обычный
    // RGBA по 8 бит; имя цветового пространства — константа AppKit.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            ptr::null_mut(),
            width,
            height,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    rep.setSize(new_size);
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;

    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    let transform = NSAffineTransform::transform();
    transform.translateXBy_yBy(new_size.width / 2.0, new_size.height / 2.0);
    transform.rotateByDegrees(degrees);
    transform.translateXBy_yBy(-new_size.width / 2.0, -new_size.height / 2.0);
    transform.concat();
    image.drawInRect_fromRect_operation_fraction(
        bounds,
        NSRect::ZERO,
        NSCompositingOperation::Copy,
        1.0,
    );
    NSGraphicsContext::restoreGraphicsState_class();

    Some(Retained::into_super(rep))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(image: &NSImage) -> Vec<(isize, isize)> {
        image
            .representations()
            .iter()
            .map(|rep| (rep.pixelsWide(), rep.pixelsHigh()))
            .collect()
    }

    #[test]
    fn item_icons_are_18_by_12_with_retina_copies() {
        let icon = action_icon(Action::LeftHalf, false);
        assert_eq!(icon.size(), ITEM_ICON_SIZE);
        assert!(icon.isTemplate());

        // У столбиков и иконки статус-бара есть файл @2x.
        let column = action_icon(Action::Column { count: 6, index: 2 }, false);
        assert_eq!(column.size(), ITEM_ICON_SIZE);
        assert_eq!(pixels(&column), vec![(18, 12), (36, 24)]);
        let status = status_icon().expect("иконка статус-бара");
        assert_eq!(status.size(), NSSize::new(22.0, 22.0));
        assert_eq!(pixels(&status), vec![(22, 22), (44, 44)]);

        // Действие без картинки — пустое место того же размера.
        let empty = action_icon(Action::TileAll, false);
        assert_eq!(empty.size(), ITEM_ICON_SIZE);
        assert!(pixels(&empty).is_empty());
    }

    #[test]
    fn thirds_rotate_in_portrait() {
        let rotated = action_icon(Action::FirstThird, true);
        let size = rotated.size();
        assert!((size.width - 12.0).abs() < 1e-9 && (size.height - 18.0).abs() < 1e-9);
        assert!(rotated.isTemplate());
        assert_eq!(pixels(&rotated), vec![(12, 18), (24, 36)]);

        // Остальные иконки на портретном экране не поворачиваются.
        assert_eq!(action_icon(Action::LeftHalf, true).size(), ITEM_ICON_SIZE);
        assert_eq!(
            action_icon(Action::FirstThird, false).size(),
            ITEM_ICON_SIZE
        );
    }

    #[test]
    fn symbols_become_bitmaps() {
        for name in ["gear", "doc.text", "arrow.down.circle"] {
            let image = symbol(name).unwrap_or_else(|| panic!("нет символа {name}"));
            assert!(image.isTemplate());
            let size = image.size();
            let reps = pixels(&image);
            assert_eq!(reps.len(), 2, "{name}");
            assert_eq!(reps[1].0, (size.width * 2.0).round() as isize, "{name}");
        }
        assert!(symbol("нет-такого-символа").is_none());
    }
}
