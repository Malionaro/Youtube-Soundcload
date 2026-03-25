import sys
import os
import json
import requests
import subprocess
import yt_dlp
import platform
import time
import threading
import webbrowser
import tarfile
import shutil
import urllib.request
import locale
import gettext
import re
import datetime
import tempfile
import traceback
import concurrent.futures
from packaging import version
from PIL import Image
from io import BytesIO

from PyQt5.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout, QGridLayout,
    QLabel, QLineEdit, QPushButton, QComboBox, QProgressBar, QTextEdit,
    QScrollArea, QFrame, QFileDialog, QMessageBox, QCheckBox, QGroupBox,
    QSizePolicy, QDialog, QPlainTextEdit
)
from PyQt5.QtCore import Qt, QSize, QTimer, QEvent
from PyQt5.QtGui import QPixmap, QIcon, QFont, QTextCursor, QPalette, QColor

# App-Konfiguration
APP_NAME = "SoundSync Downloader"
LOCAL_VERSION = "1.9.1"
GITHUB_REPO_URL = "https://github.com/Malionaro/Johann-Youtube-Soundcload"
GITHUB_API_URL = f"https://api.github.com/repos/Malionaro/Johann-Youtube-Soundcload/releases/latest"
CONFIG_PATH = "config.json"
os.environ["PATH"] += os.pathsep + "/usr/local/bin"

# Spracheinstellungen
SUPPORTED_LANGUAGES = ['en', 'de', 'pl']
DEFAULT_LANGUAGE = 'en'

# Versuche Systemsprache zu erkennen
try:
    current_locale = locale.getlocale()
    system_lang = current_locale[0] if current_locale else DEFAULT_LANGUAGE
    if system_lang:
        system_lang = system_lang.split('_')[0]
    if system_lang not in SUPPORTED_LANGUAGES:
        system_lang = DEFAULT_LANGUAGE
except:
    system_lang = DEFAULT_LANGUAGE

# Sprachdateien laden
locales_dir = os.path.join(os.path.dirname(__file__), 'locales')
lang_translations = {}

for lang in SUPPORTED_LANGUAGES:
    try:
        lang_translations[lang] = gettext.translation('app', localedir=locales_dir, languages=[lang])
    except:
        lang_translations[lang] = gettext.NullTranslations()

# Aktuelle Sprache setzen
current_language = system_lang
lang_translations[current_language].install()
_ = lang_translations[current_language].gettext

def resource_path(relative_path):
    try:
        base_path = sys._MEIPASS
    except Exception:
        base_path = os.path.abspath(".")
    return os.path.join(base_path, relative_path)

def check_ffmpeg_installed():
    try:
        result = subprocess.run(['ffmpeg', '-version'], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        return "ffmpeg version" in (result.stdout + result.stderr).lower()
    except Exception:
        return False

def install_ffmpeg(log_func=print):
    if shutil.which("ffmpeg"):
        log_func(_("✅ FFmpeg ist bereits installiert."))
        return True

    if platform.system() == "Windows":
        log_func(_("🔧 Starte FFmpeg-Installation über winget..."))
        try:
            subprocess.run(["winget", "--version"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        except (subprocess.CalledProcessError, FileNotFoundError):
            log_func(_("❌ Winget ist nicht verfügbar. Bitte FFmpeg manuell installieren."))
            return False

        try:
            result = subprocess.run(
                ["winget", "install", "--id=Gyan.FFmpeg", "-e", "--silent"],
                check=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True
            )
            log_func(result.stdout)
            log_func(_("✅ FFmpeg wurde erfolgreich installiert."))
            return True
        except subprocess.CalledProcessError as e:
            log_func(_("❌ Fehler bei der Installation von FFmpeg mit winget."))
            log_func(e.stderr)
            return False

    elif platform.system() == "Linux":
        log_func(_("🔧 Starte FFmpeg-Installation über apt..."))
        try:
            subprocess.run(['sudo', 'apt-get', 'update'], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            subprocess.run(['sudo', 'apt-get', 'install', '-y', 'ffmpeg'], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            log_func(_("✅ FFmpeg wurde erfolgreich installiert."))
            return True
        except subprocess.CalledProcessError as e:
            log_func(_("❌ Fehler bei der Installation von FFmpeg unter Linux."))
            log_func(str(e))
            return False

    elif platform.system() == "Darwin":
        log_func(_("🔧 Starte FFmpeg-Installation ohne Homebrew..."))
        if shutil.which("ffmpeg"):
            log_func(_("✅ FFmpeg ist bereits installiert."))
            return True

        try:
            # Architektur-basierte Auswahl
            machine = platform.machine().lower()
            if machine in ["arm64", "x86_64"]:
                url = f"https://evermeet.cx/ffmpeg/ffmpeg-6.0.7z"
            else:
                url = "https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz"
            
            download_dir = "/tmp/ffmpeg"
            install_dir = "/usr/local/bin"
            os.makedirs(download_dir, exist_ok=True)

            log_func(_("⬇️ Lade FFmpeg herunter von {url}...").format(url=url))
            archive_path = os.path.join(download_dir, "ffmpeg.7z" if "evermeet" in url else "ffmpeg.tar.xz")
            urllib.request.urlretrieve(url, archive_path)
            log_func(_("✅ Download abgeschlossen."))

            log_func(_("🔧 Entpacke FFmpeg..."))
            if archive_path.endswith(".7z"):
                subprocess.run(["7z", "x", archive_path, f"-o{download_dir}"], check=True)
                extracted_bin = os.path.join(download_dir, "ffmpeg")
            else:
                with tarfile.open(archive_path, "r:xz") as archive:
                    archive.extractall(download_dir)
                # Binärdatei finden
                for root, dirs, files in os.walk(download_dir):
                    if "ffmpeg" in files:
                        extracted_bin = os.path.join(root, "ffmpeg")
                        break
            
            if not os.path.exists(extracted_bin):
                raise FileNotFoundError(_("FFmpeg-Binärdatei nach dem Entpacken nicht gefunden"))
                
            ffmpeg_dest = os.path.join(install_dir, "ffmpeg")
            log_func(_("🔧 Verschiebe FFmpeg nach {install_dir}...").format(install_dir=install_dir))
            shutil.move(extracted_bin, ffmpeg_dest)
            os.chmod(ffmpeg_dest, 0o755)

            if shutil.which("ffmpeg"):
                log_func(_("✅ FFmpeg wurde erfolgreich installiert."))
                return True
            else:
                log_func(_("❌ FFmpeg konnte nicht in den PATH eingefügt werden."))
                return False

        except Exception as e:
            log_func(_("❌ Fehler bei der Installation von FFmpeg: {error}").format(error=str(e)))
            return False
    else:
        log_func(_("⚠️ Plattform nicht unterstützt."))
        return False

class YTDLogger:
    def __init__(self, app):
        self.app = app

    def debug(self, msg):
        if msg.strip():
            self.app.log(f"[DEBUG] {msg}")

    def warning(self, msg):
        self.app.log(f"[WARNUNG] {msg}")

    def error(self, msg):
        self.app.log(f"[FEHLER] {msg}")

class ChangeLogWindow(QDialog):
    def __init__(self, parent, changelog):
        super().__init__(parent)
        self.setWindowTitle(_("Änderungsprotokoll"))
        self.setGeometry(300, 300, 800, 600)
        
        layout = QVBoxLayout()
        
        title = QLabel(_("Neueste Änderungen"))
        title_font = QFont("Segoe UI", 16, QFont.Bold)
        title.setFont(title_font)
        title.setAlignment(Qt.AlignCenter)
        layout.addWidget(title)
        
        self.text_area = QPlainTextEdit()
        self.text_area.setPlainText(changelog)
        self.text_area.setReadOnly(True)
        self.text_area.setFont(QFont("Consolas", 11))
        layout.addWidget(self.text_area, 1)
        
        self.disable_check = QCheckBox(_("Änderungsprotokolle nicht mehr anzeigen"))
        layout.addWidget(self.disable_check)
        
        button_layout = QHBoxLayout()
        ok_button = QPushButton(_("OK"))
        ok_button.clicked.connect(self.accept)
        button_layout.addStretch()
        button_layout.addWidget(ok_button)
        
        layout.addLayout(button_layout)
        self.setLayout(layout)
        
        # Apply dark theme
        self.apply_dark_theme()
    
    def apply_dark_theme(self):
        palette = QPalette()
        palette.setColor(QPalette.Window, QColor(30, 30, 30))
        palette.setColor(QPalette.WindowText, Qt.white)
        palette.setColor(QPalette.Base, QColor(40, 40, 40))
        palette.setColor(QPalette.Text, Qt.white)
        palette.setColor(QPalette.Button, QColor(50, 50, 50))
        palette.setColor(QPalette.ButtonText, Qt.white)
        palette.setColor(QPalette.Highlight, QColor(42, 140, 85))
        palette.setColor(QPalette.HighlightedText, Qt.white)
        self.setPalette(palette)
    
    def accept(self):
        if self.disable_check.isChecked():
            config = {}
            if os.path.exists(CONFIG_PATH):
                try:
                    with open(CONFIG_PATH, "r") as f:
                        config = json.load(f)
                except:
                    pass
            
            config["disable_changelog"] = True
            
            with open(CONFIG_PATH, "w") as f:
                json.dump(config, f)
        
        super().accept()

class DownloaderApp(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle(APP_NAME)
        self.setGeometry(100, 100, 1200, 750)
        self.setMinimumSize(1000, 650)
        
        # Farbpalette für Themes
        self.dark_colors = {
            "bg1": "#121212",       # Tiefschwarzer Hintergrund
            "bg2": "#1E1E1E",       # Dunkelgrau für Frames
            "bg3": "#2A2A2A",       # Leicht heller für Statusbar
            "text": "white",
            "accent1": "#2A8C55",   # Grün für Download-Button
            "accent2": "#3A7EBF",   # Blau für Log leeren / Scroll-Button
            "accent3": "#C74B4B",   # Rot für Abbrechen-Button
            "accent1_hover": "#207244",
            "accent2_hover": "#2E6399",
            "accent3_hover": "#A03A3A",
            "progress1": "#3A7EBF",
            "progress2": "#2A8C55"
        }
        
        self.light_colors = {
            "bg1": "#F0F0F0",
            "bg2": "#E5E5E5",
            "bg3": "#D5D5D5",
            "text": "black",
            "accent1": "#2A8C55",
            "accent2": "#3A7EBF",
            "accent3": "#C74B4B",
            "accent1_hover": "#207244",
            "accent2_hover": "#2E6399",
            "accent3_hover": "#A03A3A",
            "progress1": "#3A7EBF",
            "progress2": "#2A8C55"
        }
        
        self.colors = self.dark_colors
        self.apply_dark_theme()
        
        # Icon setzen
        icon_path = resource_path("app_icon.ico")
        if os.path.exists(icon_path):
            self.setWindowIcon(QIcon(icon_path))
        
        # Bereinigte Formatliste
        self.codec_map = {
            "mp3": "mp3", "m4a": "m4a", "wav": "wav", "flac": "flac", "aac": "aac",
            "ogg": "vorbis", "opus": "opus", "wma": "wma", "alac": "alac", "aiff": "aiff"
        }
        self.formate = [*self.codec_map.keys(), "mp4", "webm", "mkv", "avi", "mov", "flv", "wmv", "3gp"]
        self.formate.sort()
        self.format_var = "mp3"
        self.dark_mode = True
        self.abort_event = threading.Event()
        self.is_downloading = False
        self.total_tracks = 0
        self.completed_tracks = 0
        self.successful_downloads = 0
        self.downloaded_tracks = []
        self.thumbnail_cache = {}
        self.thread_pool = concurrent.futures.ThreadPoolExecutor(max_workers=4)
        self.start_time = None
        self.last_update_time = None
        self.last_downloaded_bytes = 0
        self.current_speed = 0
        self.cookies_path = ""
        self.current_thumbnail_frame = None
        self.ydl_process = None
        self.last_gui_update = 0
        self.last_converted_file = None
        
        # Sprachauswahl
        self.current_language = current_language
        self.language_mapping = {
            'en': 'English',
            'de': 'Deutsch',
            'pl': 'Polski'
        }

        # Haupt-Widget und Layout
        central_widget = QWidget()
        self.setCentralWidget(central_widget)
        main_layout = QVBoxLayout(central_widget)
        
        # Header mit Logo und Titel
        header_frame = QFrame()
        header_frame.setObjectName("headerFrame")
        header_layout = QHBoxLayout(header_frame)
        header_layout.setContentsMargins(15, 10, 15, 10)
        
        self.logo_label = QLabel("🎵")
        self.logo_label.setFont(QFont("Arial", 24))
        self.logo_label.setFixedWidth(50)
        header_layout.addWidget(self.logo_label)
        
        self.title_label = QLabel(APP_NAME)
        self.title_label.setFont(QFont("Segoe UI", 20, QFont.Bold))
        header_layout.addWidget(self.title_label)
        
        header_layout.addStretch()
        
        self.theme_switch = QPushButton(_("Dark Mode"))
        self.theme_switch.setCheckable(True)
        self.theme_switch.setChecked(True)
        self.theme_switch.clicked.connect(self.toggle_theme)
        header_layout.addWidget(self.theme_switch)
        
        self.language_combo = QComboBox()
        for lang in SUPPORTED_LANGUAGES:
            self.language_combo.addItem(self.language_mapping[lang], lang)
        self.language_combo.setCurrentText(self.language_mapping[self.current_language])
        self.language_combo.currentIndexChanged.connect(self.change_language)
        header_layout.addWidget(self.language_combo)
        
        main_layout.addWidget(header_frame)
        
        # Hauptbereich
        main_widget = QWidget()
        main_widget_layout = QHBoxLayout(main_widget)
        main_layout.addWidget(main_widget, 1)
        
        # Linke Seite (Hauptbereich)
        left_side = QWidget()
        left_side_layout = QVBoxLayout(left_side)
        left_side_layout.setContentsMargins(5, 5, 5, 5)
        main_widget_layout.addWidget(left_side, 3)
        
        # URL-Eingabe
        url_group = QGroupBox(_("YouTube oder SoundCloud URL:"))
        url_layout = QVBoxLayout(url_group)
        
        self.url_entry = QLineEdit()
        self.url_entry.setPlaceholderText(_("https://www.youtube.com/... oder https://soundcloud.com/..."))
        self.url_entry.setMinimumHeight(40)
        url_layout.addWidget(self.url_entry)
        
        url_button_layout = QHBoxLayout()
        self.clear_url_button = QPushButton("X")
        self.clear_url_button.setFixedWidth(40)
        self.clear_url_button.clicked.connect(self.clear_url)
        url_button_layout.addStretch()
        url_button_layout.addWidget(self.clear_url_button)
        url_layout.addLayout(url_button_layout)
        
        left_side_layout.addWidget(url_group)
        
        # Zielordner und Format
        settings_group = QGroupBox(_("Einstellungen"))
        settings_layout = QGridLayout(settings_group)
        
        # Zielordner
        folder_label = QLabel(_("Zielordner:"))
        settings_layout.addWidget(folder_label, 0, 0)
        
        self.folder_entry = QLineEdit()
        self.folder_entry.setPlaceholderText(_("Wählen Sie einen Speicherort..."))
        settings_layout.addWidget(self.folder_entry, 0, 1)
        
        self.browse_button = QPushButton(_("Durchsuchen"))
        self.browse_button.clicked.connect(self.choose_folder)
        settings_layout.addWidget(self.browse_button, 0, 2)
        
        # Format
        format_label = QLabel(_("Format:"))
        settings_layout.addWidget(format_label, 1, 0)
        
        self.format_combo = QComboBox()
        self.format_combo.addItems(self.formate)
        self.format_combo.setCurrentText("mp3")
        settings_layout.addWidget(self.format_combo, 1, 1)
        
        # Cookies
        cookies_label = QLabel(_("Cookies:"))
        settings_layout.addWidget(cookies_label, 2, 0)
        
        self.cookies_entry = QLineEdit()
        self.cookies_entry.setPlaceholderText(_("Pfad zu cookies.txt"))
        settings_layout.addWidget(self.cookies_entry, 2, 1)
        
        self.cookies_button = QPushButton(_("Auswählen"))
        self.cookies_button.clicked.connect(self.choose_cookies_file)
        settings_layout.addWidget(self.cookies_button, 2, 2)
        
        left_side_layout.addWidget(settings_group)
        
        # Buttons
        button_layout = QHBoxLayout()
        
        self.download_button = QPushButton(_("Download starten"))
        self.download_button.clicked.connect(self.start_download_thread)
        self.download_button.setEnabled(False)
        button_layout.addWidget(self.download_button)
        
        self.cancel_button = QPushButton(_("Abbrechen"))
        self.cancel_button.clicked.connect(self.cancel_download)
        self.cancel_button.setEnabled(False)
        button_layout.addWidget(self.cancel_button)
        
        self.update_button = QPushButton(_("Auf Updates prüfen"))
        self.update_button.clicked.connect(lambda: threading.Thread(target=self.check_for_updates_gui).start())
        button_layout.addWidget(self.update_button)
        
        self.convert_button = QPushButton(_("Datei konvertieren"))
        self.convert_button.clicked.connect(self.open_conversion_window)
        button_layout.addWidget(self.convert_button)
        
        left_side_layout.addLayout(button_layout)
        
        # Fortschrittsbalken
        progress_group = QGroupBox(_("Fortschritt"))
        progress_layout = QVBoxLayout(progress_group)
        
        # Fortschritt für aktuellen Titel
        self.progress_label = QLabel(_("Bereit zum Starten"))
        progress_layout.addWidget(self.progress_label)
        
        self.progress = QProgressBar()
        self.progress.setValue(0)
        progress_layout.addWidget(self.progress)
        
        # Konvertierungsfortschritt
        self.convert_label = QLabel(_("Konvertierung: Wartend..."))
        progress_layout.addWidget(self.convert_label)
        
        self.convert_progress = QProgressBar()
        self.convert_progress.setValue(0)
        progress_layout.addWidget(self.convert_progress)
        
        # Gesamtfortschritt
        self.total_progress_label = QLabel(_("Gesamtfortschritt: 0% | ETA: --:--:--"))
        progress_layout.addWidget(self.total_progress_label)
        
        self.total_progress = QProgressBar()
        self.total_progress.setValue(0)
        progress_layout.addWidget(self.total_progress)
        
        left_side_layout.addWidget(progress_group)
        
        # Log-Ausgabe
        log_group = QGroupBox(_("Aktivitätsprotokoll"))
        log_layout = QVBoxLayout(log_group)
        
        log_button_layout = QHBoxLayout()
        log_button_layout.addStretch()
        
        self.clear_log_button = QPushButton(_("Log leeren"))
        self.clear_log_button.clicked.connect(self.clear_log)
        log_button_layout.addWidget(self.clear_log_button)
        
        log_layout.addLayout(log_button_layout)
        
        self.log_output = QTextEdit()
        self.log_output.setReadOnly(True)
        self.log_output.setFont(QFont("Consolas", 10))
        log_layout.addWidget(self.log_output)
        
        left_side_layout.addWidget(log_group)
        
        # Rechte Seite (Sidebar)
        right_side = QWidget()
        right_side_layout = QVBoxLayout(right_side)
        right_side_layout.setContentsMargins(5, 5, 5, 5)
        main_widget_layout.addWidget(right_side, 1)
        
        # Heruntergeladene Titel
        sidebar_title = QLabel(_("Heruntergeladene Titel:"))
        sidebar_title.setFont(QFont("Segoe UI", 11, QFont.Bold))
        right_side_layout.addWidget(sidebar_title)
        
        # Scrollable Frame für Thumbnails
        self.scroll_area = QScrollArea()
        self.scroll_area.setWidgetResizable(True)
        self.scroll_content = QWidget()
        self.scroll_layout = QVBoxLayout(self.scroll_content)
        self.scroll_layout.setAlignment(Qt.AlignTop)
        self.scroll_area.setWidget(self.scroll_content)
        right_side_layout.addWidget(self.scroll_area, 1)
        
        self.scroll_to_current_button = QPushButton(_("Zum aktuellen Titel scrollen"))
        self.scroll_to_current_button.clicked.connect(self.scroll_to_current)
        right_side_layout.addWidget(self.scroll_to_current_button)
        
        # Statusbar
        self.statusbar = self.statusBar()
        self.status_label = QLabel(_("Bereit"))
        self.statusbar.addWidget(self.status_label)
        
        self.statusbar.addPermanentWidget(QLabel(_("Version: ") + LOCAL_VERSION))
        
        github_button = QPushButton("GitHub")
        github_button.clicked.connect(lambda: webbrowser.open(GITHUB_REPO_URL))
        self.statusbar.addPermanentWidget(github_button)
        
        # Initialisierung
        self.download_folder = self.load_download_folder() or os.path.expanduser("~")
        self.folder_entry.setText(self.download_folder)
        os.makedirs(self.download_folder, exist_ok=True)
        self.update_download_button_state()
        
        # Verbindungen
        self.url_entry.textChanged.connect(self.update_download_button_state)
        self.folder_entry.textChanged.connect(self.update_download_button_state)
        
        # Beim Start: Änderungsprotokoll anzeigen, wenn nicht deaktiviert
        QTimer.singleShot(1000, self.show_changelog_on_start)
        
        # FFmpeg-Check
        QTimer.singleShot(2000, self.check_ffmpeg_installation)
        # Automatische Update-Prüfung
        QTimer.singleShot(3000, lambda: threading.Thread(target=self.check_for_updates_gui, args=(True,)).start)
    
    def apply_dark_theme(self):
        palette = QPalette()
        palette.setColor(QPalette.Window, QColor(self.colors["bg1"]))
        palette.setColor(QPalette.WindowText, QColor(self.colors["text"]))
        palette.setColor(QPalette.Base, QColor(self.colors["bg3"]))
        palette.setColor(QPalette.Text, QColor(self.colors["text"]))
        palette.setColor(QPalette.Button, QColor(self.colors["bg2"]))
        palette.setColor(QPalette.ButtonText, QColor(self.colors["text"]))
        palette.setColor(QPalette.Highlight, QColor(self.colors["accent1"]))
        palette.setColor(QPalette.HighlightedText, Qt.white)
        
        # Setze die Palette für die gesamte Anwendung
        self.setPalette(palette)
        
        # StyleSheets für spezifische Widgets
        button_style = f"""
            QPushButton {{
                background-color: {self.colors["accent1"]};
                color: white;
                border: none;
                padding: 5px 10px;
                border-radius: 4px;
            }}
            QPushButton:hover {{
                background-color: {self.colors["accent1_hover"]};
            }}
            QPushButton:disabled {{
                background-color: #555;
            }}
        """
        
        red_button_style = f"""
            QPushButton {{
                background-color: {self.colors["accent3"]};
                color: white;
                border: none;
                padding: 5px 10px;
                border-radius: 4px;
            }}
            QPushButton:hover {{
                background-color: {self.colors["accent3_hover"]};
            }}
        """
        
        blue_button_style = f"""
            QPushButton {{
                background-color: {self.colors["accent2"]};
                color: white;
                border: none;
                padding: 5px 10px;
                border-radius: 4px;
            }}
            QPushButton:hover {{
                background-color: {self.colors["accent2_hover"]};
            }}
        """
        
        convert_button_style = f"""
            QPushButton {{
                background-color: #9B59B6;
                color: white;
                border: none;
                padding: 5px 10px;
                border-radius: 4px;
            }}
            QPushButton:hover {{
                background-color: #8E44AD;
            }}
        """
        
        self.download_button.setStyleSheet(button_style)
        self.browse_button.setStyleSheet(button_style)
        self.cookies_button.setStyleSheet(button_style)
        self.cancel_button.setStyleSheet(red_button_style)
        self.update_button.setStyleSheet(blue_button_style)
        self.clear_log_button.setStyleSheet(blue_button_style)
        self.scroll_to_current_button.setStyleSheet(blue_button_style)
        self.convert_button.setStyleSheet(convert_button_style)
        self.clear_url_button.setStyleSheet(red_button_style)
        
        # Progress Bars
        progress_style = f"""
            QProgressBar {{
                border: 1px solid #444;
                border-radius: 5px;
                text-align: center;
                background-color: {self.colors["bg3"]};
            }}
            QProgressBar::chunk {{
                background-color: {self.colors["progress1"]};
                width: 10px;
            }}
        """
        
        total_progress_style = f"""
            QProgressBar {{
                border: 1px solid #444;
                border-radius: 5px;
                text-align: center;
                background-color: {self.colors["bg3"]};
            }}
            QProgressBar::chunk {{
                background-color: {self.colors["progress2"]};
                width: 10px;
            }}
        """
        
        self.progress.setStyleSheet(progress_style)
        self.convert_progress.setStyleSheet(progress_style)
        self.total_progress.setStyleSheet(total_progress_style)
        
        # Group Boxes
        group_style = f"""
            QGroupBox {{
                background-color: {self.colors["bg2"]};
                border: 1px solid #444;
                border-radius: 5px;
                margin-top: 1ex;
                font-weight: bold;
            }}
            QGroupBox::title {{
                subcontrol-origin: margin;
                left: 10px;
                padding: 0 3px 0 3px;
                color: {self.colors["text"]};
            }}
        """
        
        for group in self.findChildren(QGroupBox):
            group.setStyleSheet(group_style)
        
        # Header
        self.logo_label.setStyleSheet(f"color: {self.colors['text']};")
        self.title_label.setStyleSheet(f"color: {self.colors['text']};")
        self.status_label.setStyleSheet("color: lightgreen;")
        
        # ComboBox
        combo_style = f"""
            QComboBox {{
                background-color: {self.colors["bg3"]};
                color: {self.colors["text"]};
                border: 1px solid #444;
                padding: 2px;
                border-radius: 4px;
            }}
            QComboBox::drop-down {{
                border: none;
            }}
            QComboBox QAbstractItemView {{
                background-color: {self.colors["bg3"]};
                color: {self.colors["text"]};
                selection-background-color: {self.colors["accent1"]};
            }}
        """
        self.format_combo.setStyleSheet(combo_style)
        self.language_combo.setStyleSheet(combo_style)
    
    def cleanup_temp_files(self):
        temp_extensions = ['.part', '.tmp', '.ytdl']
        deleted = 0
        
        for root, dirs, files in os.walk(self.download_folder):
            for file in files:
                if any(file.endswith(ext) for ext in temp_extensions):
                    try:
                        os.remove(os.path.join(root, file))
                        deleted += 1
                    except Exception as e:
                        self.log(_("⚠️ Konnte temporäre Datei nicht löschen: {file} - {error}").format(file=file, error=e))
        
        if deleted > 0:
            self.log(_("🧹 {count} temporäre Dateien gelöscht").format(count=deleted))
    
    def load_download_folder(self):
        if os.path.exists(CONFIG_PATH):
            try:
                with open(CONFIG_PATH, "r") as f:
                    config = json.load(f)
                    self.cookies_path = config.get("cookies_path", "")
                    self.cookies_entry.setText(self.cookies_path)
                    
                    lang = config.get("language", system_lang)
                    if lang in SUPPORTED_LANGUAGES:
                        self.current_language = lang
                        self.language_combo.setCurrentText(self.language_mapping[lang])
                    
                    return config.get("download_folder")
            except:
                pass
        return None
    
    def save_config(self):
        config = {
            "download_folder": self.download_folder,
            "cookies_path": self.cookies_path,
            "language": self.current_language
        }
        with open(CONFIG_PATH, "w") as f:
            json.dump(config, f)
    
    def choose_folder(self):
        folder = QFileDialog.getExistingDirectory(self, _("Wählen Sie einen Zielordner"), self.download_folder)
        if folder:
            self.download_folder = folder
            self.folder_entry.setText(folder)
            self.save_config()
            self.log(_("✅ Zielordner gesetzt: {folder}").format(folder=folder))
            self.update_download_button_state()
    
    def choose_cookies_file(self):
        file_path, _ = QFileDialog.getOpenFileName(
            self,
            _("Wählen Sie eine Cookies-Datei"),
            "",
            _("Textdateien (*.txt);;Alle Dateien (*.*)")
        )
        if file_path:
            self.cookies_path = file_path
            self.cookies_entry.setText(file_path)
            self.save_config()
            self.log(_("🍪 Cookies-Datei ausgewählt: {file}").format(file=file_path))
    
    def update_download_button_state(self):
        url_filled = bool(self.url_entry.text().strip())
        folder_selected = bool(self.download_folder and os.path.isdir(self.download_folder))
        self.download_button.setEnabled(url_filled and folder_selected and not self.is_downloading)
    
    def clear_url(self):
        self.url_entry.clear()
        self.log(_("🧹 URL-Feld wurde geleert."))
        self.update_download_button_state()
    
    def clear_log(self):
        self.log_output.clear()
        self.log(_("🧹 Log wurde geleert."))
    
    def log(self, message):
        self.log_output.append(message)
        self.log_output.moveCursor(QTextCursor.End)
    
    def start_download_thread(self):
        self.is_downloading = True
        self.format_combo.setEnabled(False)
        self.abort_event.clear()
        self.cancel_button.setEnabled(True)
        self.download_button.setEnabled(False)
        self.total_tracks = 0
        self.completed_tracks = 0
        self.successful_downloads = 0
        self.downloaded_tracks = []
        self.thumbnail_cache = {}
        self.total_progress.setValue(0)
        self.convert_progress.setValue(0)
        self.start_time = time.time()
        self.last_update_time = time.time()
        self.last_downloaded_bytes = 0
        self.current_speed = 0
        self.last_gui_update = 0
        self.last_converted_file = None
        
        # Clear scrollable frame
        for i in reversed(range(self.scroll_layout.count())): 
            widget = self.scroll_layout.itemAt(i).widget()
            if widget is not None:
                widget.deleteLater()
        
        threading.Thread(target=self.download_playlist, daemon=True).start()
    
    def download_playlist(self):
        url = self.url_entry.text().strip()
        if not url:
            QMessageBox.critical(self, _("Fehler"), _("Bitte eine URL eingeben."))
            self.is_downloading = False
            return

        fmt = self.format_combo.currentText()
        self.update_status_label(_("🔍 Analysiere URL..."))
        self.log(_("🔍 Starte Analyse der URL..."))
        self.progress_label.setText(_("Analysiere URL..."))

        # Optimierte Optionen für bessere Performance
        playlist_opts = {
            'extract_flat': True,
            'playlistend': 10000,
            'ignoreerrors': True,
            'quiet': True,
            'cookiefile': self.cookies_path if self.cookies_path and os.path.exists(self.cookies_path) else None,
            'noprogress': True,  # Reduziert die Anzahl der Fortschrittsmeldungen
            'concurrent_fragment_downloads': 4,  # Parallele Fragment-Downloads
            'buffer_size': 65536,  # Größerer Puffer für bessere Performance
            'http_chunk_size': 10485760,  # 10MB Chunks für bessere Performance
        }

        if fmt in self.codec_map:
            codec = self.codec_map[fmt]
            base_opts = {
                'format': 'bestaudio/best',
                'postprocessors': [{
                    'key': 'FFmpegExtractAudio',
                    'preferredcodec': codec,
                    'preferredquality': '192'
                }],
                'progress_hooks': [self.progress_hook],
                'postprocessor_hooks': [self.progress_hook],
                'outtmpl': os.path.join(self.download_folder, '%(title)s.%(ext)s'),
                'quiet': True,
                'ignoreerrors': True,
                'noplaylist': True,
                'logger': YTDLogger(self),
                'cookiefile': self.cookies_path if self.cookies_path and os.path.exists(self.cookies_path) else None,
                'noprogress': True,
                'concurrent_fragment_downloads': 4,
                'buffer_size': 65536,
                'http_chunk_size': 10485760,
            }
        else:
            base_opts = {
                'format': 'bestvideo+bestaudio/best',
                'merge_output_format': fmt,
                'postprocessors': [{
                    'key': 'FFmpegVideoConvertor',
                    'preferedformat': fmt
                }],
                'progress_hooks': [self.progress_hook],
                'postprocessor_hooks': [self.progress_hook],
                'outtmpl': os.path.join(self.download_folder, '%(title)s.%(ext)s'),
                'quiet': True,
                'ignoreerrors': True,
                'noplaylist': True,
                'logger': YTDLogger(self),
                'cookiefile': self.cookies_path if self.cookies_path and os.path.exists(self.cookies_path) else None,
                'noprogress': True,
                'concurrent_fragment_downloads': 4,
                'buffer_size': 65536,
                'http_chunk_size': 10485760,
            }

        try:
            # Step 1: Playlist-Informationen extrahieren
            with yt_dlp.YoutubeDL({**playlist_opts, 'logger': YTDLogger(self)}) as ydl:
                info = ydl.extract_info(url, download=False)
                
            if 'entries' in info:
                entries = info['entries']
            else:
                entries = [info]
                
            self.total_tracks = len(entries)
            self.log(_("📂 {count} Titel gefunden.").format(count=self.total_tracks))
            self.progress_label.setText(_("{count} Titel gefunden").format(count=self.total_tracks))
            self.update_total_progress()

            # Step 2: Titel einzeln herunterladen
            for i, e in enumerate(entries, 1):
                if self.abort_event.is_set():
                    self.update_status_label(_("❌ Abgebrochen"))
                    self.log(_("🛑 Der Download wurde abgebrochen."))
                    break

                title = e.get('title', _("Track {number}").format(number=i))
                link = e.get('url') or e.get('webpage_url', url)
                thumbnail = e.get('thumbnail') or e.get('thumbnails', [{}])[0].get('url') if isinstance(e.get('thumbnails'), list) else None

                # Thumbnail im Hintergrund laden
                if thumbnail:
                    self.thread_pool.submit(self.load_thumbnail, thumbnail, title, i)

                self.update_status_label(_("⬇️ Lade: {title} ({current}/{total})").format(
                    title=title, current=i, total=self.total_tracks))
                self.log(_("⬇️ {current}/{total} – {title}").format(
                    current=i, total=self.total_tracks, title=title))
                self.progress_label.setText(_("Lade Titel {current}/{total}").format(
                    current=i, total=self.total_tracks))
                self.progress.setValue(0)
                self.convert_progress.setValue(0)
                self.convert_label.setText(_("Konvertierung: Wartend..."))

                try:
                    with yt_dlp.YoutubeDL(base_opts) as ydl:
                        ydl.download([link])
                    self.successful_downloads += 1
                    self.downloaded_tracks.append(title)
                except yt_dlp.utils.DownloadError as e:
                    if _("Download abgebrochen") in str(e):
                        self.log(_("🛑 Download abgebrochen: {title}").format(title=title))
                        break
                    elif "Sign in to confirm you're not a bot" in str(e):
                        error_msg = _(
                            "❌ YouTube verlangt Bestätigung, dass Sie kein Bot sind.\n\n"
                            "Bitte verwenden Sie die Cookies-Funktion:\n"
                            "1. Installieren Sie den 'Get Cookies.txt' Browser-Addon\n"
                            "2. Exportieren Sie Cookies von youtube.com\n"
                            "3. Wählen Sie die cookies.txt-Datei im Tool aus"
                        )
                        self.log(error_msg)
                        QMessageBox.critical(self, _("YouTube Bot-Erkennung"), error_msg)
                        break
                    else:
                        self.log(_("⚠️ Fehler beim Laden von {title}: {error}").format(title=title, error=e))
                except Exception as e:
                    self.log(_("⚠️ Unerwarteter Fehler beim Laden von {title}: {error}").format(title=title, error=e))
                finally:
                    self.completed_tracks = i
                    self.update_total_progress()

                if self.abort_event.is_set():
                    self.update_status_label(_("❌ Abgebrochen"))
                    self.log(_("🛑 Der Download wurde abgebrochen."))
                    break

                # Kurze Pause zwischen Downloads
                time.sleep(0.2)

        except Exception as e:
            self.update_status_label(_("❌ Fehler aufgetreten"))
            self.progress_label.setText(_("Fehler: {error}...").format(error=str(e)[:50]))
            self.log(_("❌ Fehler beim Download: {error}").format(error=e))
            QMessageBox.critical(self, _("Fehler"), _("Fehler beim Download:\n{error}").format(error=e))
        finally:
            self.format_combo.setEnabled(True)
            self.cancel_button.setEnabled(False)
            self.download_button.setEnabled(True)
            self.is_downloading = False
            self.update_download_button_state()

            # Bereinige temporäre Dateien nach Abbruch
            if self.abort_event.is_set():
                self.cleanup_temp_files()

        if not self.abort_event.is_set() and not any((self.abort_event.is_set(), "Fehler" in self.status_label.text())):
            self.update_status_label(_("✅ Download abgeschlossen!"))
            self.progress_label.setText(_("Alle Downloads abgeschlossen"))
            
            success_rate = (self.successful_downloads / self.total_tracks * 100) if self.total_tracks > 0 else 0
            self.log(_("🎉 Download abgeschlossen: {success} von {total} Titeln erfolgreich geladen.").format(
                success=self.successful_downloads, total=self.total_tracks))
            self.log(_("📊 Erfolgsrate: {rate:.1f}%").format(rate=success_rate))
            
            list_path = os.path.join(self.download_folder, "download_list.txt")
            with open(list_path, 'w', encoding='utf-8') as f:
                for idx, title in enumerate(self.downloaded_tracks, 1):
                    f.write(f"{idx}. {title}\n")
            self.log(_("📝 Liste der heruntergeladenen Titel gespeichert: {path}").format(path=list_path))
            
            message = _(
                "Download abgeschlossen!\n\n"
                "Erfolgreich geladene Titel: {success} von {total}\n"
                "Erfolgsrate: {rate:.1f}%\n\n"
                "Eine Liste der Titel wurde gespeichert unter:\n{path}"
            ).format(success=self.successful_downloads, total=self.total_tracks, rate=success_rate, path=list_path)
            QMessageBox.information(self, _("Fertig"), message)
            
            self.cleanup_temp_files()
    
    def progress_hook(self, d):
        if self.abort_event.is_set():
            raise yt_dlp.utils.DownloadError(_("Download abgebrochen"))
            
        current_time = time.time()
        time_diff = current_time - self.last_update_time
        
        # GUI-Update nur alle 100 ms, um Performance zu verbessern
        update_gui = False
        if current_time - self.last_gui_update > 0.1 or d['status'] == 'finished':
            update_gui = True
            self.last_gui_update = current_time

        if d['status'] == 'downloading':
            total = d.get('total_bytes') or d.get('total_bytes_estimate')
            downloaded = d.get('downloaded_bytes', 0)
            
            if total:
                percent = int(downloaded / total * 100)
                self.progress.setValue(percent)
                
                if time_diff > 0.5:
                    downloaded_diff = downloaded - self.last_downloaded_bytes
                    self.current_speed = downloaded_diff / time_diff
                    self.last_downloaded_bytes = downloaded
                    self.last_update_time = current_time
                
                if update_gui:
                    if self.current_speed > 0:
                        remaining_bytes = total - downloaded
                        eta_seconds = remaining_bytes / self.current_speed
                        eta_str = str(datetime.timedelta(seconds=int(eta_seconds)))
                        self.progress_label.setText(_("Fortschritt: {percent}% | Geschw: {speed} | ETA: {eta}").format(
                            percent=percent, speed=self.format_speed(self.current_speed), eta=eta_str))
                    else:
                        self.progress_label.setText(_("Fortschritt: {percent}% | Geschw: berechne...").format(percent=percent))
            else:
                if update_gui:
                    if time_diff > 0.5:
                        downloaded_diff = downloaded - self.last_downloaded_bytes
                        self.current_speed = downloaded_diff / time_diff
                        self.last_downloaded_bytes = downloaded
                        self.last_update_time = current_time
                    self.progress_label.setText(_("Läuft... | Geschw: {speed}").format(speed=self.format_speed(self.current_speed)))
            
        elif d['status'] == 'finished' and update_gui:
            self.progress.setValue(100)
            self.progress_label.setText(_("✅ Download abgeschlossen"))
            self.log(_("✅ Download abgeschlossen."))
        
        # Konvertierungsfortschritt
        if d.get('postprocessor') and d.get('postprocessor') in ['FFmpegExtractAudio', 'FFmpegVideoConvertor']:
            filename = d.get('info_dict', {}).get('filepath', _('Unbekannte Datei'))
            if filename:
                # Nur bei Dateiwechsel oder neuem Fortschritt aktualisieren
                if filename != self.last_converted_file or d.get('postprocessor_progress', 0) == 0:
                    short_filename = os.path.basename(filename)
                    self.convert_label.setText(_("Konvertierung: {file}").format(file=short_filename))
                    self.last_converted_file = filename
            
            if d.get('postprocessor_progress') is not None:
                progress = int(d['postprocessor_progress'] * 100)
                self.convert_progress.setValue(progress)
                
                if d['status'] == 'finished' and update_gui:
                    self.convert_progress.setValue(100)
                    self.convert_label.setText(_("✅ Konvertierung abgeschlossen"))
    
    def format_speed(self, speed_bytes):
        if speed_bytes < 1024:
            return _("{bytes:.1f} B/s").format(bytes=speed_bytes)
        elif speed_bytes < 1024 * 1024:
            return _("{kb:.1f} KB/s").format(kb=speed_bytes / 1024)
        else:
            return _("{mb:.1f} MB/s").format(mb=speed_bytes / (1024 * 1024))
    
    def update_status_label(self, text):
        self.status_label.setText(text)
    
    def update_total_progress(self):
        if self.total_tracks > 0:
            progress_value = self.completed_tracks / self.total_tracks
            percent = int(progress_value * 100)
            self.total_progress.setValue(percent)
            
            if self.start_time and self.completed_tracks > 0:
                elapsed_time = time.time() - self.start_time
                avg_time_per_track = elapsed_time / self.completed_tracks
                remaining_tracks = self.total_tracks - self.completed_tracks
                total_eta_seconds = remaining_tracks * avg_time_per_track
                eta_str = str(datetime.timedelta(seconds=int(total_eta_seconds)))
                self.total_progress_label.setText(
                    _("Gesamtfortschritt: {percent}% | {completed}/{total} Titel | ETA: {eta}").format(
                        percent=percent, completed=self.completed_tracks, total=self.total_tracks, eta=eta_str)
                )
            else:
                self.total_progress_label.setText(
                    _("Gesamtfortschritt: {percent}% | {completed}/{total} Titel | ETA: berechne...").format(
                        percent=percent, completed=self.completed_tracks, total=self.total_tracks)
                )
        else:
            self.total_progress.setValue(0)
            self.total_progress_label.setText(_("Gesamtfortschritt: 0% | ETA: --:--:--"))
    
    def load_thumbnail(self, url, title, index):
        try:
            if url in self.thumbnail_cache:
                self.add_thumbnail(self.thumbnail_cache[url], title, index)
                return
                
            response = requests.get(url, timeout=10)
            img = Image.open(BytesIO(response.content))
            img = img.resize((120, 90), Image.LANCZOS)
            pixmap = QPixmap.fromImage(ImageQt(img))
            
            self.add_thumbnail(pixmap, title, index)
            self.thumbnail_cache[url] = pixmap
            return pixmap
        except Exception as e:
            self.log(_("⚠️ Thumbnail-Fehler für '{title}': {error}").format(title=title, error=e))
            return None
    
    def add_thumbnail(self, pixmap, title, index):
        if self.abort_event.is_set():
            return
            
        frame = QFrame()
        frame.setFrameShape(QFrame.StyledPanel)
        frame.setMinimumHeight(100)
        frame.setMaximumHeight(100)
        frame.setStyleSheet("border: 1px solid #444; border-radius: 4px;")
        
        layout = QHBoxLayout(frame)
        
        label_img = QLabel()
        label_img.setPixmap(pixmap)
        label_img.setFixedSize(120, 90)
        layout.addWidget(label_img)
        
        label_title = QLabel(f"{index}. {title}")
        label_title.setWordWrap(True)
        label_title.setMaximumWidth(150)
        layout.addWidget(label_title)
        
        self.scroll_layout.addWidget(frame)
        
        if index == self.completed_tracks + 1:
            frame.setStyleSheet("border: 2px solid #2A8C55; border-radius: 4px;")
            self.current_thumbnail_frame = frame
    
    def cancel_download(self):
        if self.is_downloading:
            reply = QMessageBox.question(
                self,
                _("Download abbrechen"),
                _("Möchten Sie den aktuellen Download wirklich abbrechen?"),
                QMessageBox.Yes | QMessageBox.No,
                QMessageBox.No
            )
            
            if reply == QMessageBox.Yes:
                self.abort_event.set()
                self.is_downloading = False
                
                if self.ydl_process:
                    try:
                        self.ydl_process.terminate()
                        self.log(_("🔴 Prozess wurde gewaltsam beendet."))
                    except:
                        pass
                
                self.log(_("🛑 Download sofort abgebrochen"))
                self.status_label.setText(_("❌ Abgebrochen"))
                self.cancel_button.setEnabled(False)
                self.cleanup_temp_files()
        else:
            self.log(_("ℹ️ Es läuft kein Download, der abgebrochen werden könnte."))
    
    def scroll_to_current(self):
        if self.current_thumbnail_frame:
            self.scroll_area.verticalScrollBar().setValue(
                self.scroll_area.verticalScrollBar().maximum()
            )
    
    def open_conversion_window(self):
        ConversionWindow(self, self.download_folder).exec_()
    
    def check_for_updates_gui(self, auto_check=False):
        try:
            if not auto_check:
                self.log(_("🔍 Suche nach Updates..."))
                self.status_label.setText(_("🔍 Suche nach Updates..."))
            
            headers = {
                "User-Agent": "SoundSync-Downloader"
            }
            
            r = requests.get(GITHUB_API_URL, headers=headers, timeout=10)
            r.raise_for_status()
            
            try:
                latest = r.json()
            except json.JSONDecodeError:
                self.log(_("⚠️ Ungültige JSON-Antwort: {text}...").format(text=r.text[:100]))
                if not auto_check:
                    self.status_label.setText(_("⚠️ Update-Prüfung fehlgeschlagen"))
                return
                
            ver = latest.get("tag_name", "").lstrip("v")
            
            version_match = re.search(r'\d+\.\d+\.\d+', ver)
            if not version_match:
                self.log(_("⚠️ Keine gültige Version gefunden"))
                if not auto_check:
                    self.status_label.setText(_("⚠️ Update-Prüfung fehlgeschlagen"))
                return
                
            ver = version_match.group(0)
            
            if version.parse(ver) > version.parse(LOCAL_VERSION):
                self.log(_("⬆️ Neue Version verfügbar: {version}").format(version=ver))
                self.status_label.setText(_("⬆️ Update verfügbar: v{version}").format(version=ver))
                
                if not auto_check or QMessageBox.question(
                    self,
                    _("Update verfügbar"), 
                    _("Version {version} verfügbar. Jetzt herunterladen?").format(version=ver),
                    QMessageBox.Yes | QMessageBox.No
                ) == QMessageBox.Yes:
                    self.download_update(latest)
            else:
                if not auto_check:
                    self.log(_("✅ Keine neue Version gefunden."))
                    self.status_label.setText(_("✅ Aktuelle Version"))
        except requests.exceptions.RequestException as e:
            self.log(_("⚠️ Netzwerkfehler bei Update-Prüfung: {error}").format(error=e))
            if not auto_check:
                self.status_label.setText(_("⚠️ Update-Prüfung fehlgeschlagen"))
        except Exception as e:
            self.log(_("⚠️ Update-Fehler: {error}").format(error=e))
            if not auto_check:
                self.status_label.setText(_("⚠️ Update-Prüfung fehlgeschlagen"))
    
    def download_update(self, release_info):
        asset = None
        for a in release_info.get('assets', []):
            if "win" in a['name'].lower() and "exe" in a['name'].lower():
                asset = a
                break
            elif "linux" in a['name'].lower() and "tar" in a['name'].lower():
                asset = a
                break
            elif "mac" in a['name'].lower() and "dmg" in a['name'].lower():
                asset = a
                break
        
        if not asset:
            self.log(_("❌ Keine passende Installationsdatei gefunden"))
            QMessageBox.critical(self, _("Fehler"), _("Keine passende Installationsdatei für dieses System gefunden."))
            return
        
        download_url = asset['browser_download_url']
        self.log(_("⬇️ Lade Update herunter von: {url}").format(url=download_url))
        
        UpdateWindow(self, download_url).exec_()
    
    def change_language(self, index):
        lang_code = self.language_combo.itemData(index)
        
        if not lang_code:
            return
            
        global current_language, _
        
        if lang_code in SUPPORTED_LANGUAGES:
            current_language = lang_code
            lang_translations[current_language].install()
            _ = lang_translations[current_language].gettext
            
            self.save_config()
            self.log(_("🌐 Sprache geändert zu: {language}").format(language=current_language))
            
            # UI aktualisieren
            self.refresh_ui()
    
    def refresh_ui(self):
        """Aktualisiert alle UI-Elemente mit neuen Übersetzungen"""
        self.setWindowTitle(APP_NAME)
        self.theme_switch.setText(_("Dark Mode"))
        self.title_label.setText(APP_NAME)
        self.url_entry.setPlaceholderText(_("https://www.youtube.com/... oder https://soundcloud.com/..."))
        self.folder_entry.setPlaceholderText(_("Wählen Sie einen Speicherort..."))
        self.browse_button.setText(_("Durchsuchen"))
        self.format_combo.setToolTip(_("Format"))
        self.cookies_entry.setPlaceholderText(_("Pfad zu cookies.txt"))
        self.cookies_button.setText(_("Auswählen"))
        self.download_button.setText(_("Download starten"))
        self.cancel_button.setText(_("Abbrechen"))
        self.update_button.setText(_("Auf Updates prüfen"))
        self.convert_button.setText(_("Datei konvertieren"))
        self.progress_label.setText(_("Bereit zum Starten"))
        self.convert_label.setText(_("Konvertierung: Wartend..."))
        self.total_progress_label.setText(_("Gesamtfortschritt: 0% | ETA: --:--:--"))
        self.clear_log_button.setText(_("Log leeren"))
        self.scroll_to_current_button.setText(_("Zum aktuellen Titel scrollen"))
        self.status_label.setText(_("Bereit"))
        
        # Sprachauswahl aktualisieren
        for i in range(self.language_combo.count()):
            lang_code = self.language_combo.itemData(i)
            self.language_combo.setItemText(i, self.language_mapping[lang_code])
    
    def show_changelog_on_start(self):
        disable_changelog = False
        if os.path.exists(CONFIG_PATH):
            try:
                with open(CONFIG_PATH, "r") as f:
                    config = json.load(f)
                    disable_changelog = config.get("disable_changelog", False)
            except:
                pass
        
        if not disable_changelog:
            changelog = _("""Version 1.9.1 - Änderungsprotokoll

Neue Funktionen:
- Verbesserter Dark Mode mit tiefschwarzem Design
- Optimierte Performance für lange Downloads
- Stabilerer Konvertierungsfortschrittsbalken
- Vollständige Übersetzung für alle Sprachen

Verbesserungen:
- Reduzierte GUI-Blockierungen während des Downloads
- Bessere Fehlerbehandlung bei Konvertierungen
- Schnellere Thumbnail-Ladezeiten
- Effizientere Speichernutzung

Fehlerbehebungen:
- Konvertierungsbalken zeigt jetzt korrekten Fortschritt
- Sprachausgabe für alle Elemente konsistent
- Diverse Stabilitätsverbesserungen""")
            
            ChangeLogWindow(self, changelog).exec_()
    
    def toggle_theme(self):
        self.dark_mode = self.theme_switch.isChecked()
        self.colors = self.dark_colors if self.dark_mode else self.light_colors
        self.apply_dark_theme()
    
    def check_ffmpeg_installation(self):
        if not check_ffmpeg_installed():
            reply = QMessageBox.question(
                self,
                _("FFmpeg fehlt"),
                _("⚠️ FFmpeg ist nicht installiert. Möchten Sie es jetzt installieren?"),
                QMessageBox.Yes | QMessageBox.No
            )
            
            if reply == QMessageBox.Yes:
                self.status_label.setText(_("🔧 Installiere FFmpeg..."))
                success = install_ffmpeg(self.log)
                if success:
                    self.status_label.setText(_("✅ FFmpeg installiert"))
                else:
                    self.status_label.setText(_("❌ FFmpeg-Installation fehlgeschlagen"))
                    QMessageBox.critical(
                        self,
                        _("Installation fehlgeschlagen"),
                        _("❌ FFmpeg konnte nicht installiert werden. Bitte manuell installieren.")
                    )
            else:
                self.status_label.setText(_("⚠️ FFmpeg benötigt"))
                QMessageBox.warning(
                    self,
                    _("FFmpeg benötigt"),
                    _("❗ Ohne FFmpeg funktioniert der Download nicht korrekt.")
                )

class UpdateWindow(QDialog):
    def __init__(self, parent, download_url):
        super().__init__(parent)
        self.setWindowTitle(_("Update wird installiert"))
        self.setGeometry(300, 300, 600, 400)
        self.download_url = download_url
        
        layout = QVBoxLayout(self)
        
        title = QLabel(_("Software-Update"))
        title.setFont(QFont("Segoe UI", 16, QFont.Bold))
        title.setAlignment(Qt.AlignCenter)
        layout.addWidget(title)
        
        self.status_label = QLabel(_("Vorbereitung..."))
        self.status_label.setAlignment(Qt.AlignCenter)
        layout.addWidget(self.status_label)
        
        self.progress = QProgressBar()
        self.progress.setValue(0)
        layout.addWidget(self.progress)
        
        self.console = QPlainTextEdit()
        self.console.setReadOnly(True)
        self.console.setFont(QFont("Consolas", 10))
        layout.addWidget(self.console, 1)
        
        # Apply dark theme
        self.apply_dark_theme()
        
        threading.Thread(target=self.download_and_install, daemon=True).start()
    
    def apply_dark_theme(self):
        palette = QPalette()
        palette.setColor(QPalette.Window, QColor(30, 30, 30))
        palette.setColor(QPalette.WindowText, Qt.white)
        palette.setColor(QPalette.Base, QColor(40, 40, 40))
        palette.setColor(QPalette.Text, Qt.white)
        palette.setColor(QPalette.Button, QColor(50, 50, 50))
        palette.setColor(QPalette.ButtonText, Qt.white)
        self.setPalette(palette)
    
    def log(self, message):
        self.console.appendPlainText(message)
    
    def download_and_install(self):
        try:
            temp_dir = tempfile.mkdtemp()
            download_path = os.path.join(temp_dir, "update_package")
            
            if platform.system() == "Windows":
                download_path += ".exe"
            elif platform.system() == "Darwin":
                download_path += ".dmg"
            else:
                download_path += ".tar.gz"
            
            self.status_label.setText(_("Lade Update herunter..."))
            self.log(_("⬇️ Starte Download von: {}").format(self.download_url))
            
            with requests.get(self.download_url, stream=True) as r:
                r.raise_for_status()
                total_size = int(r.headers.get('content-length', 0))
                downloaded = 0
                
                with open(download_path, 'wb') as f:
                    for chunk in r.iter_content(chunk_size=8192):
                        if chunk:
                            f.write(chunk)
                            downloaded += len(chunk)
                            
                            progress = downloaded / total_size * 100 if total_size else 0
                            self.progress.setValue(int(progress))
                            self.log(_("⬇️ Heruntergeladen: {downloaded}/{total} Bytes ({percent:.1%})").format(
                                downloaded=downloaded, total=total_size, percent=progress/100))
            
            self.log(_("✅ Download abgeschlossen"))
            self.status_label.setText(_("Installiere Update..."))
            
            if platform.system() == "Windows":
                self.log(_("🔧 Starte Installationsprogramm..."))
                subprocess.Popen([download_path], shell=True)
                
                self.log(_("🔄 Beende Anwendung für Update..."))
                QApplication.quit()
                
            elif platform.system() == "Darwin":
                self.log(_("🔧 Installiere auf macOS..."))
                self.log(_("❌ macOS-Installation noch nicht implementiert"))
                self.status_label.setText(_("Fehler: Nicht implementiert"))
                
            else:
                self.log(_("🔧 Installiere auf Linux..."))
                self.log(_("❌ Linux-Installation noch nicht implementiert"))
                self.status_label.setText(_("Fehler: Nicht implementiert"))
            
        except Exception as e:
            self.log(_("❌ Fehler bei der Update-Installation: {}").format(str(e)))
            self.log(traceback.format_exc())
            self.status_label.setText(_("Fehler bei der Installation"))

class ConversionWindow(QDialog):
    def __init__(self, parent, download_folder):
        super().__init__(parent)
        self.setWindowTitle(_("Konvertierung"))
        self.setGeometry(300, 300, 500, 400)
        self.download_folder = download_folder
        self.file_path = ""
        
        layout = QVBoxLayout(self)
        
        title = QLabel(_("🔧 Dateikonvertierung"))
        title.setFont(QFont("Segoe UI", 16, QFont.Bold))
        title.setAlignment(Qt.AlignCenter)
        layout.addWidget(title)
        
        # Dateiauswahl
        file_layout = QHBoxLayout()
        self.file_entry = QLineEdit()
        self.file_entry.setPlaceholderText(_("Wählen Sie eine Datei..."))
        file_layout.addWidget(self.file_entry)
        
        self.browse_button = QPushButton(_("Durchsuchen"))
        self.browse_button.clicked.connect(self.choose_file)
        file_layout.addWidget(self.browse_button)
        
        layout.addLayout(file_layout)
        
        # Format und Qualität
        settings_layout = QGridLayout()
        
        format_label = QLabel(_("Zielformat:"))
        settings_layout.addWidget(format_label, 0, 0)
        
        self.format_combo = QComboBox()
        self.format_combo.addItems([
            "mp3", "wav", "flac", "m4a", "ogg", "aac", 
            "mp4", "avi", "mov", "mkv", "flv",
            "jpg", "jpeg", "png", "bmp", "gif", "tiff", "webp",
            "pdf", "docx", "pptx", "xlsx", "txt", "html"
        ])
        settings_layout.addWidget(self.format_combo, 0, 1)
        
        quality_label = QLabel(_("Qualität:"))
        settings_layout.addWidget(quality_label, 1, 0)
        
        self.quality_combo = QComboBox()
        self.quality_combo.addItems([_("Niedrig"), _("Mittel"), _("Hoch"), _("Maximal")])
        self.quality_combo.setCurrentText(_("Hoch"))
        settings_layout.addWidget(self.quality_combo, 1, 1)
        
        layout.addLayout(settings_layout)
        
        # Konvertierungs-Button
        self.convert_button = QPushButton(_("Konvertierung starten"))
        self.convert_button.clicked.connect(self.start_conversion)
        layout.addWidget(self.convert_button)
        
        # Status und Fortschritt
        self.status_label = QLabel(_("Bereit zur Konvertierung"))
        self.status_label.setAlignment(Qt.AlignCenter)
        layout.addWidget(self.status_label)
        
        self.progress = QProgressBar()
        self.progress.setTextVisible(False)
        layout.addWidget(self.progress)
        
        # Info-Label
        info_label = QLabel(_("ℹ️ Hochwertige Konvertierung mit erhaltener Qualität"))
        info_label.setAlignment(Qt.AlignCenter)
        layout.addWidget(info_label)
        
        # Apply premium theme
        self.apply_premium_theme()
    
    def apply_premium_theme(self):
        palette = QPalette()
        palette.setColor(QPalette.Window, QColor(44, 62, 80))  # #2C3E50
        palette.setColor(QPalette.WindowText, QColor(236, 240, 241))  # #ECF0F1
        palette.setColor(QPalette.Base, QColor(52, 73, 94))  # #34495e
        palette.setColor(QPalette.Text, QColor(236, 240, 241))  # #ECF0F1
        palette.setColor(QPalette.Button, QColor(155, 89, 182))  # #9B59B6
        palette.setColor(QPalette.ButtonText, Qt.white)
        palette.setColor(QPalette.Highlight, QColor(142, 68, 173))  # #8E44AD
        self.setPalette(palette)
        
        # Button Style
        button_style = """
            QPushButton {
                background-color: #9B59B6;
                color: white;
                border: none;
                padding: 8px 16px;
                border-radius: 4px;
                font-weight: bold;
            }
            QPushButton:hover {
                background-color: #8E44AD;
            }
        """
        self.convert_button.setStyleSheet(button_style)
        self.browse_button.setStyleSheet(button_style)
        
        # Progress Bar Style
        progress_style = """
            QProgressBar {
                border: 1px solid #444;
                border-radius: 5px;
                background-color: #2C3E50;
            }
            QProgressBar::chunk {
                background-color: #3498DB;
            }
        """
        self.progress.setStyleSheet(progress_style)
    
    def choose_file(self):
        file_path, _ = QFileDialog.getOpenFileName(
            self,
            _("Datei auswählen"),
            self.download_folder,
            _("Alle Dateien (*.*)")
        )
        
        if file_path:
            self.file_path = file_path
            self.file_entry.setText(file_path)
            
            try:
                file_type = "Unbekannter Typ"
                try:
                    import filetype
                    kind = filetype.guess(file_path)
                    if kind is not None:
                        file_type = kind.mime
                except ImportError:
                    # Fallback ohne filetype
                    ext = os.path.splitext(file_path)[1].lower()
                    if ext:
                        file_type = ext[1:].upper() + " Datei"
            except Exception:
                file_type = _("Unbekannter Typ")
            
            self.status_label.setText(
                _("Datei ausgewählt: {file} ({type})").format(
                    file=os.path.basename(file_path), type=file_type)
            )
            self.status_label.setStyleSheet("color: #2ECC71;")  # Grün
    
    def start_conversion(self):
        if not self.file_path or not os.path.exists(self.file_path):
            QMessageBox.critical(self, _("Fehler"), _("Bitte wählen Sie eine gültige Datei aus."))
            return
            
        threading.Thread(target=self.convert_file, daemon=True).start()
    
    def convert_file(self):
        try:
            self.progress.setRange(0, 0)  # Indeterminate mode
            self.status_label.setText(_("Konvertierung läuft..."))
            self.status_label.setStyleSheet("color: #3498DB;")  # Blau
            
            base_name = os.path.splitext(os.path.basename(self.file_path))[0]
            target_format = self.format_combo.currentText()
            output_path = os.path.join(
                os.path.dirname(self.file_path),
                f"{base_name}_konvertiert.{target_format}"
            )
            
            # Stelle sicher, dass das Ausgabeverzeichnis existiert
            os.makedirs(os.path.dirname(output_path), exist_ok=True)
            
            quality = self.quality_combo.currentText()
            quality_args = []
            
            if quality == _("Niedrig"):
                quality_args = ["-compression_level", "1"]
            elif quality == _("Mittel"):
                quality_args = ["-compression_level", "5"]
            elif quality == _("Hoch"):
                quality_args = ["-compression_level", "8"]
            elif quality == _("Maximal"):
                quality_args = ["-compression_level", "12"]
            
            cmd = [
                'ffmpeg', 
                '-i', self.file_path,
                '-y'  # Überschreiben ohne Nachfrage
            ] + quality_args + [output_path]
            
            process = subprocess.Popen(
                cmd,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                universal_newlines=True,
                encoding='utf-8',
                errors='replace'
            )
            
            # Warte auf Abschluss des Prozesses
            process.communicate()
            
            if process.returncode == 0:
                self.status_label.setText(
                    _("✅ Konvertierung abgeschlossen: {file}").format(file=os.path.basename(output_path)))
                self.status_label.setStyleSheet("color: #2ECC71;")  # Grün
                QMessageBox.information(
                    self,
                    _("Konvertierung erfolgreich"),
                    _("Datei wurde erfolgreich konvertiert:\n{path}").format(path=output_path)
                )
            else:
                self.status_label.setText(_("❌ Konvertierung fehlgeschlagen"))
                self.status_label.setStyleSheet("color: #E74C3C;")  # Rot
                QMessageBox.critical(
                    self,
                    _("Fehler"),
                    _("Konvertierung fehlgeschlagen. Bitte überprüfen Sie die Datei und das Format.")
                )
        except Exception as e:
            self.status_label.setText(_("❌ Fehler: {error}").format(error=str(e)))
            self.status_label.setStyleSheet("color: #E74C3C;")  # Rot
        finally:
            self.progress.setRange(0, 100)  # Zurück zu determinate mode
            self.progress.setValue(0)

if __name__ == "__main__":
    app = QApplication(sys.argv)
    window = DownloaderApp()
    window.show()
    sys.exit(app.exec_())