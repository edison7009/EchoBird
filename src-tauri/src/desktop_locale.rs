pub(crate) fn language_name(locale: &str) -> Option<&'static str> {
    match locale {
        "zh" | "zh-Hans" => Some("Simplified Chinese"),
        "zh-Hant" => Some("Traditional Chinese"),
        "en" => Some("English"),
        "ja" => Some("Japanese"),
        "ko" => Some("Korean"),
        "es" => Some("Spanish"),
        "fr" => Some("French"),
        "de" => Some("German"),
        "pt" => Some("Portuguese"),
        "ru" => Some("Russian"),
        "vi" => Some("Vietnamese"),
        _ => None,
    }
}

pub(crate) struct DesktopLabels {
    pub(crate) show: &'static str,
    pub(crate) quit: &'static str,
    // Settings, Edit, View, Window.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) mac_menu: [&'static str; 4],
}

pub(crate) fn desktop_labels(locale: &str) -> DesktopLabels {
    let labels = match locale {
        "zh-Hans" => ["显示 EchoBird", "退出", "设置…", "编辑", "视图", "窗口"],
        "zh-Hant" => ["顯示 EchoBird", "結束", "設定…", "編輯", "顯示方式", "視窗"],
        "ja" => [
            "EchoBirdを表示",
            "終了",
            "設定…",
            "編集",
            "表示",
            "ウインドウ",
        ],
        "ko" => ["EchoBird 보기", "종료", "설정…", "편집", "보기", "윈도우"],
        "es" => [
            "Mostrar EchoBird",
            "Salir",
            "Ajustes…",
            "Edición",
            "Visualización",
            "Ventana",
        ],
        "fr" => [
            "Afficher EchoBird",
            "Quitter",
            "Réglages…",
            "Édition",
            "Présentation",
            "Fenêtre",
        ],
        "de" => [
            "EchoBird anzeigen",
            "Beenden",
            "Einstellungen…",
            "Bearbeiten",
            "Darstellung",
            "Fenster",
        ],
        "pt" => [
            "Mostrar EchoBird",
            "Sair",
            "Definições…",
            "Editar",
            "Visualização",
            "Janela",
        ],
        "ru" => [
            "Показать EchoBird",
            "Выход",
            "Настройки…",
            "Правка",
            "Вид",
            "Окно",
        ],
        "vi" => ["Hiện EchoBird", "Thoát", "Cài đặt…", "Sửa", "Xem", "Cửa sổ"],
        _ => [
            "Show EchoBird",
            "Quit",
            "Settings…",
            "Edit",
            "View",
            "Window",
        ],
    };
    let [show, quit, settings, edit, view, window] = labels;
    DesktopLabels {
        show,
        quit,
        mac_menu: [settings, edit, view, window],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_app_language_has_a_language_name_and_tray_labels() {
        let fallback = desktop_labels("en");
        for locale in [
            "zh-Hans", "zh-Hant", "ja", "ko", "es", "fr", "de", "pt", "ru", "vi",
        ] {
            let labels = desktop_labels(locale);
            assert!(language_name(locale).is_some());
            assert_ne!(labels.show, fallback.show, "{locale}");
            assert_ne!(labels.quit, fallback.quit, "{locale}");
        }
        assert_eq!(desktop_labels("unknown").show, fallback.show);
    }
}
