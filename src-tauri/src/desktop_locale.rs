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
    // Settings, Feedback, Edit, View, Window, Help.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) mac_menu: [&'static str; 6],
}

pub(crate) fn desktop_labels(locale: &str) -> DesktopLabels {
    let labels = match locale {
        "zh-Hans" => [
            "显示 EchoBird",
            "退出",
            "设置…",
            "问题反馈",
            "编辑",
            "视图",
            "窗口",
            "帮助",
        ],
        "zh-Hant" => [
            "顯示 EchoBird",
            "結束",
            "設定…",
            "意見回饋",
            "編輯",
            "顯示方式",
            "視窗",
            "輔助說明",
        ],
        "ja" => [
            "EchoBirdを表示",
            "終了",
            "設定…",
            "フィードバック",
            "編集",
            "表示",
            "ウインドウ",
            "ヘルプ",
        ],
        "ko" => [
            "EchoBird 보기",
            "종료",
            "설정…",
            "의견 보내기",
            "편집",
            "보기",
            "윈도우",
            "도움말",
        ],
        "es" => [
            "Mostrar EchoBird",
            "Salir",
            "Ajustes…",
            "Enviar comentarios",
            "Edición",
            "Visualización",
            "Ventana",
            "Ayuda",
        ],
        "fr" => [
            "Afficher EchoBird",
            "Quitter",
            "Réglages…",
            "Envoyer des commentaires",
            "Édition",
            "Présentation",
            "Fenêtre",
            "Aide",
        ],
        "de" => [
            "EchoBird anzeigen",
            "Beenden",
            "Einstellungen…",
            "Feedback",
            "Bearbeiten",
            "Darstellung",
            "Fenster",
            "Hilfe",
        ],
        "pt" => [
            "Mostrar EchoBird",
            "Sair",
            "Definições…",
            "Enviar feedback",
            "Editar",
            "Visualização",
            "Janela",
            "Ajuda",
        ],
        "ru" => [
            "Показать EchoBird",
            "Выход",
            "Настройки…",
            "Обратная связь",
            "Правка",
            "Вид",
            "Окно",
            "Справка",
        ],
        "vi" => [
            "Hiện EchoBird",
            "Thoát",
            "Cài đặt…",
            "Gửi phản hồi",
            "Sửa",
            "Xem",
            "Cửa sổ",
            "Trợ giúp",
        ],
        _ => [
            "Show EchoBird",
            "Quit",
            "Settings…",
            "Feedback",
            "Edit",
            "View",
            "Window",
            "Help",
        ],
    };
    let [show, quit, settings, feedback, edit, view, window, help] = labels;
    DesktopLabels {
        show,
        quit,
        mac_menu: [settings, feedback, edit, view, window, help],
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
