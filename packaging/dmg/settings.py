# Настройки dmgbuild для Rectangle2Rust.dmg — вызывается из packaging/release.sh:
#   dmgbuild -s packaging/dmg/settings.py -D app=<путь к .app> "Rectangle 2 (Rust)" dist/Rectangle2Rust.dmg
# Пути — от корня репозитория. Фон — пара background.png + background@2x.png:
# dmgbuild сам склеит их в TIFF для Retina.
import os.path

app = defines["app"]
name = os.path.basename(app)

format = "ULFO"
filesystem = "HFS+"
files = [app]
# Ярлык на /Applications: в Finder его подпись — «Программы», как у настоящей папки.
symlinks = {"Программы": "/Applications"}
hide_extensions = [name]
icon = "packaging/AppIcon.icns"
background = "packaging/dmg/background.png"

# Высота окна — 400 точек фона плюс заголовок окна (32 точки в macOS 26+).
# Фон на 40 точек выше: где заголовок ниже, снизу видна та же заливка.
# Начало координат Finder — левый нижний угол экрана.
window_rect = ((400, 300), (640, 432))
default_view = "icon-view"
show_icon_preview = False
icon_size = 112
text_size = 13
label_pos = "bottom"
# Центры значков — под стрелку на фоне.
icon_locations = {name: (160, 190), "Программы": (480, 190)}
