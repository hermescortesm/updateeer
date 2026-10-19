//! Interface languages. The code is written in Spanish, so a Spanish text is also the key of its
//! translations: `t("Ajustes")` returns the text for the chosen language, and falls back to
//! English and then to the Spanish original when a translation is missing.
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Es,
    En,
    Pt,
    Fr,
    De,
    Ja,
    Zh,
}

impl Lang {
    pub const ALL: [Lang; 7] = [Lang::Es, Lang::En, Lang::Pt, Lang::Fr, Lang::De, Lang::Ja, Lang::Zh];

    /// Name in its own language, as shown in the pickers.
    pub fn native(self) -> &'static str {
        match self {
            Lang::Es => "Español",
            Lang::En => "English",
            Lang::Pt => "Português",
            Lang::Fr => "Français",
            Lang::De => "Deutsch",
            Lang::Ja => "日本語",
            Lang::Zh => "简体中文",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Es => "es",
            Lang::En => "en",
            Lang::Pt => "pt",
            Lang::Fr => "fr",
            Lang::De => "de",
            Lang::Ja => "ja",
            Lang::Zh => "zh",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        let code = code.trim().to_lowercase();
        Lang::ALL.into_iter().find(|l| code.starts_with(l.code()))
    }

    /// Index into a translation row (`[en, pt, fr, de, ja, zh]`); Spanish is the key itself.
    fn column(self) -> Option<usize> {
        match self {
            Lang::Es => None,
            Lang::En => Some(0),
            Lang::Pt => Some(1),
            Lang::Fr => Some(2),
            Lang::De => Some(3),
            Lang::Ja => Some(4),
            Lang::Zh => Some(5),
        }
    }
}

/// The system language, from the usual environment variables (`es_MX.UTF-8`, `pt-BR`, ...) or
/// the OS preferences on macOS and Windows; English when unknown.
pub fn detect() -> Lang {
    let from_env = ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"].iter().filter_map(|v| std::env::var(v).ok()).find_map(|v| Lang::from_code(&v));
    from_env.or_else(os_language).unwrap_or(Lang::En)
}

fn os_language() -> Option<Lang> {
    let out = if cfg!(target_os = "macos") {
        std::process::Command::new("/usr/bin/defaults").args(["read", "-g", "AppleLanguages"]).output().ok()?
    } else if cfg!(windows) {
        let mut cmd = std::process::Command::new("powershell");
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000);
        cmd.args(["-NoProfile", "-Command", "(Get-Culture).Name"]).output().ok()?
    } else {
        return None;
    };
    String::from_utf8_lossy(&out.stdout).split(|c: char| !c.is_alphanumeric() && c != '-').find_map(Lang::from_code)
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    CURRENT.store(Lang::ALL.iter().position(|l| *l == lang).unwrap_or(0) as u8, Ordering::Relaxed);
}

pub fn get() -> Lang {
    Lang::ALL[(CURRENT.load(Ordering::Relaxed) as usize).min(Lang::ALL.len() - 1)]
}

fn index() -> &'static HashMap<&'static str, &'static [&'static str; 6]> {
    static INDEX: OnceLock<HashMap<&'static str, &'static [&'static str; 6]>> = OnceLock::new();
    INDEX.get_or_init(|| TABLE.iter().map(|(es, row)| (*es, row)).collect())
}

/// The text in the chosen language. Text that is not in the table comes back unchanged.
pub fn t(es: &str) -> &str {
    let Some(col) = get().column() else { return es };
    match index().get(es) {
        Some(row) if !row[col].is_empty() => row[col],
        Some(row) => row[0],
        None => es,
    }
}

/// `t` plus values for the `{0}`, `{1}` ... placeholders.
pub fn tf(es: &str, values: &[String]) -> String {
    let mut text = t(es).to_string();
    for (i, v) in values.iter().enumerate() {
        text = text.replace(&format!("{{{i}}}"), v);
    }
    text
}

/// `tf` with a single value for `{0}`.
pub fn tf1(es: &str, value: &str) -> String {
    t(es).replace("{0}", value)
}

pub fn months() -> [&'static str; 12] {
    match get() {
        Lang::Es => ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"],
        Lang::En => ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
        Lang::Pt => ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"],
        Lang::Fr => ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."],
        Lang::De => ["Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.", "Dez."],
        Lang::Ja => ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
        Lang::Zh => ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
    }
}

/// `[en, pt, fr, de, ja, zh]` for each Spanish text.
const TABLE: &[(&str, [&str; 6])] = &[
    // Sidebar
    ("Actualizaciones", ["Updates", "Atualizações", "Mises à jour", "Updates", "アップデート", "更新"]),
    ("Instaladas", ["Installed", "Instalados", "Installées", "Installiert", "インストール済み", "已安装"]),
    ("Descubrir", ["Discover", "Descobrir", "Découvrir", "Entdecken", "見つける", "发现"]),
    ("Historial", ["History", "Histórico", "Historique", "Verlauf", "履歴", "历史记录"]),
    ("Ajustes", ["Settings", "Ajustes", "Réglages", "Einstellungen", "設定", "设置"]),
    ("Cada {0} h, con la app abierta", ["Every {0} h, while the app is open", "A cada {0} h, com o app aberto", "Toutes les {0} h, app ouverte", "Alle {0} Std., solange die App offen ist", "アプリを開いている間、{0}時間ごと", "应用打开时，每 {0} 小时"]),
    ("Desactivada", ["Off", "Desativada", "Désactivée", "Aus", "オフ", "已关闭"]),
    ("Búsqueda automática", ["Automatic search", "Busca automática", "Recherche automatique", "Automatische Suche", "自動検索", "自动检查"]),
    ("Versión {0}", ["Version {0}", "Versão {0}", "Version {0}", "Version {0}", "バージョン {0}", "版本 {0}"]),
    ("Nueva versión {0} disponible", ["New version {0} available", "Nova versão {0} disponível", "Nouvelle version {0} disponible", "Neue Version {0} verfügbar", "新バージョン {0} があります", "有新版本 {0}"]),
    ("Ver en Ajustes", ["See in Settings", "Ver em Ajustes", "Voir dans les Réglages", "In den Einstellungen ansehen", "設定で見る", "在设置中查看"]),
    ("Abrir la página de descarga", ["Open the download page", "Abrir a página de download", "Ouvrir la page de téléchargement", "Download-Seite öffnen", "ダウンロードページを開く", "打开下载页面"]),
    // Updates page
    ("Buscando actualizaciones…", ["Looking for updates…", "Buscando atualizações…", "Recherche de mises à jour…", "Suche nach Updates …", "アップデートを確認中…", "正在检查更新…"]),
    ("No se pudo buscar: {0}", ["Could not search: {0}", "Não foi possível buscar: {0}", "Recherche impossible : {0}", "Suche fehlgeschlagen: {0}", "検索できませんでした: {0}", "无法检查：{0}"]),
    ("{0} disponibles{1}", ["{0} available{1}", "{0} disponíveis{1}", "{0} disponibles{1}", "{0} verfügbar{1}", "{0} 件利用可能{1}", "{0} 个可用{1}"]),
    ("Actualizando…", ["Updating…", "Atualizando…", "Mise à jour…", "Wird aktualisiert …", "アップデート中…", "正在更新…"]),
    ("{0} fallidas", ["{0} failed", "{0} com falha", "{0} échouées", "{0} fehlgeschlagen", "{0} 件失敗", "{0} 个失败"]),
    ("Todo al día", ["Everything is up to date", "Tudo atualizado", "Tout est à jour", "Alles aktuell", "すべて最新です", "全部已是最新"]),
    ("Actualizar todo", ["Update all", "Atualizar tudo", "Tout mettre à jour", "Alle aktualisieren", "すべて更新", "全部更新"]),
    ("Buscando…", ["Searching…", "Buscando…", "Recherche…", "Suche …", "検索中…", "搜索中…"]),
    ("Buscar actualizaciones", ["Check for updates", "Buscar atualizações", "Rechercher des mises à jour", "Nach Updates suchen", "アップデートを確認", "检查更新"]),
    ("Tipo", ["Type", "Tipo", "Type", "Typ", "種類", "类型"]),
    ("Todo", ["All", "Tudo", "Tout", "Alle", "すべて", "全部"]),
    ("Apps", ["Apps", "Apps", "Apps", "Apps", "アプリ", "应用"]),
    ("Herramientas", ["Tools", "Ferramentas", "Outils", "Werkzeuge", "ツール", "工具"]),
    ("Instalada", ["Installed", "Instalado", "Installée", "Installiert", "インストール日", "安装时间"]),
    ("Cualquiera", ["Any", "Qualquer", "Toutes", "Beliebig", "指定なし", "不限"]),
    ("Últimos 7 días", ["Last 7 days", "Últimos 7 dias", "7 derniers jours", "Letzte 7 Tage", "過去7日間", "最近 7 天"]),
    ("Últimos 30 días", ["Last 30 days", "Últimos 30 dias", "30 derniers jours", "Letzte 30 Tage", "過去30日間", "最近 30 天"]),
    ("Hace más de 3 meses", ["Over 3 months ago", "Há mais de 3 meses", "Il y a plus de 3 mois", "Vor über 3 Monaten", "3か月以上前", "3 个月以前"]),
    ("Orden", ["Sort", "Ordem", "Tri", "Sortierung", "並べ替え", "排序"]),
    ("Nombre", ["Name", "Nome", "Nom", "Name", "名前", "名称"]),
    ("Más recientes", ["Newest", "Mais recentes", "Plus récentes", "Neueste", "新しい順", "最新优先"]),
    ("Más antiguas", ["Oldest", "Mais antigas", "Plus anciennes", "Älteste", "古い順", "最早优先"]),
    ("Limpiar", ["Clear", "Limpar", "Effacer", "Zurücksetzen", "クリア", "清除"]),
    ("Todas", ["All", "Todas", "Toutes", "Alle", "すべて", "全部"]),
    ("Directas", ["Direct", "Diretas", "Directes", "Direkt", "直接", "直接安装"]),
    ("Seleccionar todas", ["Select all", "Selecionar todas", "Tout sélectionner", "Alle auswählen", "すべて選択", "全选"]),
    ("1 seleccionada", ["1 selected", "1 selecionada", "1 sélectionnée", "1 ausgewählt", "1 件選択", "已选 1 项"]),
    ("{0} seleccionadas", ["{0} selected", "{0} selecionadas", "{0} sélectionnées", "{0} ausgewählt", "{0} 件選択", "已选 {0} 项"]),
    ("Actualizar seleccionadas", ["Update selected", "Atualizar selecionadas", "Mettre à jour la sélection", "Auswahl aktualisieren", "選択を更新", "更新所选"]),
    ("No hay actualizaciones.", ["No updates.", "Nenhuma atualização.", "Aucune mise à jour.", "Keine Updates.", "アップデートはありません。", "没有可用更新。"]),
    ("Ninguna app coincide con tu búsqueda.", ["No app matches your search.", "Nenhum app corresponde à busca.", "Aucune app ne correspond à votre recherche.", "Keine App passt zur Suche.", "検索に一致するアプリはありません。", "没有符合搜索的应用。"]),
    // Release banner
    ("Updateeer {0} está disponible", ["Updateeer {0} is available", "Updateeer {0} está disponível", "Updateeer {0} est disponible", "Updateeer {0} ist verfügbar", "Updateeer {0} が利用可能です", "Updateeer {0} 已发布"]),
    ("No se pudo actualizar: {0}", ["Could not update: {0}", "Não foi possível atualizar: {0}", "Mise à jour impossible : {0}", "Aktualisierung fehlgeschlagen: {0}", "アップデートできませんでした: {0}", "无法更新：{0}"]),
    ("La app se reiniciará sola al terminar.", ["The app will restart by itself when it finishes.", "O app reinicia sozinho ao terminar.", "L'app redémarrera toute seule à la fin.", "Die App startet nach dem Abschluss selbst neu.", "完了するとアプリは自動で再起動します。", "完成后应用会自动重启。"]),
    ("Tienes la {0}. Se descarga, se verifica y la app se reinicia sola.", ["You have {0}. It is downloaded, verified and the app restarts by itself.", "Você tem a {0}. Ela é baixada, verificada e o app reinicia sozinho.", "Vous avez la {0}. Elle est téléchargée, vérifiée et l'app redémarre toute seule.", "Sie haben {0}. Es wird geladen, geprüft und die App startet selbst neu.", "現在は {0} です。ダウンロードと検証のあと、アプリが自動で再起動します。", "当前为 {0}。将自动下载、校验并重启应用。"]),
    ("Tienes la {0}. Descárgala desde la página de la versión.", ["You have {0}. Download it from the release page.", "Você tem a {0}. Baixe pela página da versão.", "Vous avez la {0}. Téléchargez-la depuis la page de la version.", "Sie haben {0}. Laden Sie es von der Release-Seite.", "現在は {0} です。リリースページからダウンロードしてください。", "当前为 {0}。请从发布页面下载。"]),
    ("Instalar y reiniciar", ["Install and restart", "Instalar e reiniciar", "Installer et redémarrer", "Installieren und neu starten", "インストールして再起動", "安装并重启"]),
    ("Descargar", ["Download", "Baixar", "Télécharger", "Herunterladen", "ダウンロード", "下载"]),
    ("Ocultar novedades", ["Hide what's new", "Ocultar novidades", "Masquer les nouveautés", "Neuigkeiten ausblenden", "新機能を隠す", "隐藏更新内容"]),
    ("Novedades", ["What's new", "Novidades", "Nouveautés", "Neuigkeiten", "新機能", "更新内容"]),
    // Setup banners
    ("Falta Homebrew", ["Homebrew is missing", "Falta o Homebrew", "Homebrew est absent", "Homebrew fehlt", "Homebrew がありません", "缺少 Homebrew"]),
    ("Updateeer lo usa para buscar y actualizar tus apps. Se instala desde Terminal y te pedirá tu contraseña.", ["Updateeer uses it to find and update your apps. It installs from Terminal and will ask for your password.", "O Updateeer o usa para buscar e atualizar seus apps. É instalado pelo Terminal e pedirá sua senha.", "Updateeer l'utilise pour chercher et mettre à jour vos apps. Il s'installe depuis le Terminal et demandera votre mot de passe.", "Updateeer nutzt es, um Ihre Apps zu finden und zu aktualisieren. Es wird im Terminal installiert und fragt nach Ihrem Passwort.", "Updateeer はこれを使ってアプリを検索・更新します。ターミナルからインストールされ、パスワードを求められます。", "Updateeer 用它来查找和更新应用。将在“终端”中安装，并需要输入密码。"]),
    ("Instalar Homebrew", ["Install Homebrew", "Instalar o Homebrew", "Installer Homebrew", "Homebrew installieren", "Homebrew をインストール", "安装 Homebrew"]),
    ("Falta mas para las apps de la App Store", ["mas is missing for App Store apps", "Falta o mas para os apps da App Store", "mas est absent pour les apps de l'App Store", "mas fehlt für App-Store-Apps", "App Store アプリ用の mas がありません", "缺少用于 App Store 应用的 mas"]),
    ("Es una herramienta pequeña que se instala con Homebrew. Sin ella no se ven las actualizaciones de la App Store.", ["It is a small tool installed with Homebrew. Without it, App Store updates are not shown.", "É uma ferramenta pequena instalada com o Homebrew. Sem ela, as atualizações da App Store não aparecem.", "C'est un petit outil installé avec Homebrew. Sans lui, les mises à jour de l'App Store n'apparaissent pas.", "Ein kleines Werkzeug, das mit Homebrew installiert wird. Ohne es erscheinen keine App-Store-Updates.", "Homebrew でインストールする小さなツールです。ないと App Store のアップデートが表示されません。", "这是一个通过 Homebrew 安装的小工具。没有它就看不到 App Store 的更新。"]),
    ("Instalar mas", ["Install mas", "Instalar o mas", "Installer mas", "mas installieren", "mas をインストール", "安装 mas"]),
    ("Instalando…", ["Installing…", "Instalando…", "Installation…", "Wird installiert …", "インストール中…", "正在安装…"]),
    ("Ahora no", ["Not now", "Agora não", "Pas maintenant", "Nicht jetzt", "あとで", "暂不"]),
    ("Actualizadas {0} de {1}", ["Updated {0} of {1}", "Atualizados {0} de {1}", "{0} sur {1} mises à jour", "{0} von {1} aktualisiert", "{1} 件中 {0} 件を更新済み", "已更新 {0}/{1}"]),
    ("Cancelar", ["Cancel", "Cancelar", "Annuler", "Abbrechen", "キャンセル", "取消"]),
    ("Buscar app…", ["Search apps…", "Buscar app…", "Rechercher une app…", "App suchen …", "アプリを検索…", "搜索应用…"]),
    // List rows
    ("App", ["App", "App", "App", "App", "アプリ", "应用"]),
    ("Versión", ["Version", "Versão", "Version", "Version", "バージョン", "版本"]),
    ("Origen", ["Source", "Origem", "Source", "Quelle", "提供元", "来源"]),
    ("Falló: {0}", ["Failed: {0}", "Falhou: {0}", "Échec : {0}", "Fehlgeschlagen: {0}", "失敗: {0}", "失败：{0}"]),
    ("Instalada el {0}", ["Installed on {0}", "Instalado em {0}", "Installée le {0}", "Installiert am {0}", "{0} にインストール", "安装于 {0}"]),
    ("Actualizar", ["Update", "Atualizar", "Mettre à jour", "Aktualisieren", "更新", "更新"]),
    ("Error desconocido", ["Unknown error", "Erro desconhecido", "Erreur inconnue", "Unbekannter Fehler", "不明なエラー", "未知错误"]),
    ("Reintentar", ["Retry", "Tentar de novo", "Réessayer", "Erneut versuchen", "再試行", "重试"]),
    ("En cola", ["Queued", "Na fila", "En attente", "In Warteschlange", "待機中", "排队中"]),
    ("Descargando", ["Downloading", "Baixando", "Téléchargement", "Lädt herunter", "ダウンロード中", "下载中"]),
    ("Instalando", ["Installing", "Instalando", "Installation", "Installiert", "インストール中", "安装中"]),
    ("Actualizada", ["Updated", "Atualizado", "Mise à jour faite", "Aktualisiert", "更新済み", "已更新"]),
    ("Verificando", ["Verifying", "Verificando", "Vérification", "Prüft", "検証中", "校验中"]),
    // Discover / installed / detail
    ("Apps populares para instalar con un clic", ["Popular apps to install in one click", "Apps populares para instalar com um clique", "Apps populaires à installer en un clic", "Beliebte Apps zum Installieren mit einem Klick", "ワンクリックでインストールできる人気アプリ", "一键安装的热门应用"]),
    ("Hace falta un gestor de paquetes (Homebrew, winget o Flatpak) para instalar apps.", ["A package manager (Homebrew, winget or Flatpak) is needed to install apps.", "É necessário um gerenciador de pacotes (Homebrew, winget ou Flatpak) para instalar apps.", "Un gestionnaire de paquets (Homebrew, winget ou Flatpak) est nécessaire pour installer des apps.", "Zum Installieren von Apps wird ein Paketmanager (Homebrew, winget oder Flatpak) benötigt.", "アプリをインストールするにはパッケージマネージャー（Homebrew、winget、Flatpak）が必要です。", "安装应用需要包管理器（Homebrew、winget 或 Flatpak）。"]),
    ("Categoría", ["Category", "Categoria", "Catégorie", "Kategorie", "カテゴリ", "类别"]),
    ("Buscar más resultados en línea", ["Search more results online", "Buscar mais resultados online", "Chercher plus de résultats en ligne", "Mehr Ergebnisse online suchen", "オンラインでさらに検索", "在线搜索更多结果"]),
    ("Nada coincide. Prueba «Buscar más resultados en línea».", ["Nothing matches. Try “Search more results online”.", "Nada corresponde. Tente «Buscar mais resultados online».", "Aucun résultat. Essayez « Chercher plus de résultats en ligne ».", "Keine Treffer. Versuchen Sie „Mehr Ergebnisse online suchen“.", "一致するものがありません。「オンラインでさらに検索」をお試しください。", "没有匹配项。试试“在线搜索更多结果”。"]),
    ("Instalar", ["Install", "Instalar", "Installer", "Installieren", "インストール", "安装"]),
    ("Leyendo tus apps…", ["Reading your apps…", "Lendo seus apps…", "Lecture de vos apps…", "Apps werden gelesen …", "アプリを読み込み中…", "正在读取应用…"]),
    ("{0} apps instaladas", ["{0} installed apps", "{0} apps instalados", "{0} apps installées", "{0} installierte Apps", "インストール済み {0} 件", "已安装 {0} 个应用"]),
    ("No se pudo leer: {0}", ["Could not read: {0}", "Não foi possível ler: {0}", "Lecture impossible : {0}", "Lesen fehlgeschlagen: {0}", "読み込めませんでした: {0}", "无法读取：{0}"]),
    ("Cargando…", ["Loading…", "Carregando…", "Chargement…", "Wird geladen …", "読み込み中…", "加载中…"]),
    ("No se encontraron apps instaladas.", ["No installed apps found.", "Nenhum app instalado encontrado.", "Aucune app installée trouvée.", "Keine installierten Apps gefunden.", "インストール済みのアプリが見つかりません。", "未找到已安装的应用。"]),
    ("Volver", ["Back", "Voltar", "Retour", "Zurück", "戻る", "返回"]),
    ("app (cask)", ["app (cask)", "app (cask)", "app (cask)", "App (Cask)", "アプリ (cask)", "应用 (cask)"]),
    ("app de la tienda", ["store app", "app da loja", "app de la boutique", "Store-App", "ストアアプリ", "商店应用"]),
    ("descarga directa", ["direct download", "download direto", "téléchargement direct", "Direktdownload", "直接ダウンロード", "直接下载"]),
    ("fórmula", ["formula", "fórmula", "formule", "Formel", "フォーミュラ", "公式 (formula)"]),
    ("Actualización disponible", ["Update available", "Atualização disponível", "Mise à jour disponible", "Update verfügbar", "アップデートあり", "有可用更新"]),
    ("Actualizando {0}%", ["Updating {0}%", "Atualizando {0}%", "Mise à jour {0} %", "Aktualisiert {0} %", "更新中 {0}%", "正在更新 {0}%"]),
    ("Falló", ["Failed", "Falhou", "Échec", "Fehlgeschlagen", "失敗", "失败"]),
    ("Estado", ["Status", "Estado", "État", "Status", "状態", "状态"]),
    ("Versión instalada", ["Installed version", "Versão instalada", "Version installée", "Installierte Version", "インストール済みバージョン", "已安装版本"]),
    ("Versión nueva", ["New version", "Nova versão", "Nouvelle version", "Neue Version", "新バージョン", "新版本"]),
    ("Tamaño de descarga", ["Download size", "Tamanho do download", "Taille du téléchargement", "Downloadgröße", "ダウンロードサイズ", "下载大小"]),
    ("Paquete", ["Package", "Pacote", "Paquet", "Paket", "パッケージ", "软件包"]),
    ("Comando", ["Command", "Comando", "Commande", "Befehl", "コマンド", "命令"]),
    ("Último error", ["Last error", "Último erro", "Dernière erreur", "Letzter Fehler", "直近のエラー", "最近的错误"]),
    ("Todavía no has actualizado nada", ["You haven't updated anything yet", "Você ainda não atualizou nada", "Vous n'avez encore rien mis à jour", "Sie haben noch nichts aktualisiert", "まだ何も更新していません", "你还没有更新任何内容"]),
    ("{0} actualizaciones registradas", ["{0} updates recorded", "{0} atualizações registradas", "{0} mises à jour enregistrées", "{0} Updates protokolliert", "{0} 件の更新を記録", "已记录 {0} 次更新"]),
    ("Aquí aparecerán las apps que actualices.", ["The apps you update will appear here.", "Os apps que você atualizar aparecerão aqui.", "Les apps que vous mettrez à jour apparaîtront ici.", "Hier erscheinen die Apps, die Sie aktualisieren.", "更新したアプリがここに表示されます。", "你更新的应用会显示在这里。"]),
    // Welcome
    ("Bienvenido a Updateeer", ["Welcome to Updateeer", "Bem-vindo ao Updateeer", "Bienvenue dans Updateeer", "Willkommen bei Updateeer", "Updateeer へようこそ", "欢迎使用 Updateeer"]),
    ("Antes de actualizar nada, comprueba que todo está listo", ["Before updating anything, check that everything is ready", "Antes de atualizar, confira se tudo está pronto", "Avant de mettre à jour, vérifiez que tout est prêt", "Prüfen Sie vor dem Aktualisieren, ob alles bereit ist", "更新する前に、準備が整っているか確認してください", "更新之前，请确认一切就绪"]),
    ("Modificar apps", ["Modify apps", "Modificar apps", "Modifier les apps", "Apps ändern", "アプリの変更", "修改应用"]),
    ("macOS pide el permiso «Gestión de apps» para que Updateeer pueda actualizar las apps de Aplicaciones. Actívalo en Ajustes del Sistema → Privacidad y seguridad.", ["macOS asks for the “App Management” permission so Updateeer can update the apps in Applications. Turn it on in System Settings → Privacy & Security.", "O macOS pede a permissão «Gerenciamento de apps» para o Updateeer atualizar os apps em Aplicativos. Ative em Ajustes do Sistema → Privacidade e Segurança.", "macOS demande l'autorisation « Gestion des apps » pour qu'Updateeer puisse mettre à jour les apps dans Applications. Activez-la dans Réglages Système → Confidentialité et sécurité.", "macOS verlangt die Berechtigung „App-Verwaltung“, damit Updateeer die Apps im Ordner Programme aktualisieren kann. Aktivieren Sie sie in Systemeinstellungen → Datenschutz & Sicherheit.", "Updateeer が「アプリケーション」内のアプリを更新できるよう、macOS は「アプリの管理」の許可を求めます。システム設定 → プライバシーとセキュリティ で有効にしてください。", "macOS 需要“应用管理”权限，Updateeer 才能更新“应用程序”中的应用。请在 系统设置 → 隐私与安全性 中开启。"]),
    ("Listo", ["Ready", "Pronto", "Prêt", "Bereit", "準備完了", "已就绪"]),
    ("Abrir ajustes", ["Open settings", "Abrir ajustes", "Ouvrir les réglages", "Einstellungen öffnen", "設定を開く", "打开设置"]),
    ("Comprobar", ["Check", "Verificar", "Vérifier", "Prüfen", "確認", "检查"]),
    ("Falta", ["Missing", "Falta", "Absent", "Fehlt", "未導入", "缺少"]),
    ("Sin comprobar", ["Not checked", "Não verificado", "Non vérifié", "Nicht geprüft", "未確認", "未检查"]),
    ("Lo usa para buscar y actualizar apps y herramientas. Se instala desde Terminal y pide tu contraseña.", ["It uses it to find and update apps and tools. It installs from Terminal and asks for your password.", "Usa para buscar e atualizar apps e ferramentas. É instalado pelo Terminal e pede sua senha.", "Il l'utilise pour chercher et mettre à jour apps et outils. Il s'installe depuis le Terminal et demande votre mot de passe.", "Wird zum Suchen und Aktualisieren von Apps und Werkzeugen genutzt. Die Installation läuft im Terminal und fragt nach Ihrem Passwort.", "アプリやツールの検索・更新に使います。ターミナルからインストールされ、パスワードを求められます。", "用于查找和更新应用与工具。在“终端”中安装，并需要输入密码。"]),
    ("Instalado", ["Installed", "Instalado", "Installé", "Installiert", "インストール済み", "已安装"]),
    ("mas (App Store)", ["mas (App Store)", "mas (App Store)", "mas (App Store)", "mas (App Store)", "mas (App Store)", "mas (App Store)"]),
    ("Herramienta pequeña, se instala con Homebrew. Sin ella no se ven las actualizaciones de la App Store.", ["A small tool installed with Homebrew. Without it, App Store updates are not shown.", "Ferramenta pequena instalada com o Homebrew. Sem ela, as atualizações da App Store não aparecem.", "Petit outil installé avec Homebrew. Sans lui, les mises à jour de l'App Store n'apparaissent pas.", "Ein kleines Werkzeug, das mit Homebrew installiert wird. Ohne es erscheinen keine App-Store-Updates.", "Homebrew でインストールする小さなツールです。ないと App Store のアップデートが表示されません。", "通过 Homebrew 安装的小工具。没有它就看不到 App Store 的更新。"]),
    ("Necesita Homebrew", ["Needs Homebrew", "Precisa do Homebrew", "Nécessite Homebrew", "Benötigt Homebrew", "Homebrew が必要です", "需要 Homebrew"]),
    ("Continuar", ["Continue", "Continuar", "Continuer", "Weiter", "続ける", "继续"]),
    ("Puedes volver aquí desde Ajustes → Permisos.", ["You can come back here from Settings → Permissions.", "Você pode voltar aqui em Ajustes → Permissões.", "Vous pouvez revenir ici depuis Réglages → Autorisations.", "Sie können jederzeit über Einstellungen → Berechtigungen hierher zurückkehren.", "設定 → 権限 からいつでもここに戻れます。", "你可以随时在 设置 → 权限 中回到这里。"]),
    // Settings
    ("Cómo busca Updateeer", ["How Updateeer looks", "Como o Updateeer busca", "Comment Updateeer cherche", "Wie Updateeer sucht", "Updateeer の検索方法", "Updateeer 的检查方式"]),
    ("Busca actualizaciones sola mientras la app esté abierta.", ["Looks for updates by itself while the app is open.", "Busca atualizações sozinho enquanto o app estiver aberto.", "Cherche seule des mises à jour tant que l'app est ouverte.", "Sucht selbst nach Updates, solange die App geöffnet ist.", "アプリを開いている間、自動でアップデートを確認します。", "应用打开期间自动检查更新。"]),
    ("Actualizar solas las apps cerradas", ["Update closed apps automatically", "Atualizar sozinho os apps fechados", "Mettre à jour seules les apps fermées", "Geschlossene Apps automatisch aktualisieren", "閉じているアプリを自動で更新", "自动更新已关闭的应用"]),
    ("Las que estén abiertas te avisan con una notificación: actualizar ahora o más tarde.", ["Open ones notify you: update now or later.", "Os que estiverem abertos avisam com uma notificação: atualizar agora ou depois.", "Celles qui sont ouvertes vous préviennent par une notification : maintenant ou plus tard.", "Geöffnete Apps melden sich per Benachrichtigung: jetzt oder später aktualisieren.", "開いているアプリは通知でお知らせします。今すぐ更新するか、あとで更新するか選べます。", "已打开的应用会通过通知提醒：立即更新或稍后。"]),
    ("Avisar de versiones nuevas de Updateeer", ["Notify about new Updateeer versions", "Avisar sobre novas versões do Updateeer", "Signaler les nouvelles versions d'Updateeer", "Über neue Updateeer-Versionen informieren", "Updateeer の新バージョンを通知", "提示 Updateeer 新版本"]),
    ("Consulta GitHub al buscar actualizaciones. Es la única conexión que hace la app por su cuenta.", ["Checks GitHub when looking for updates. It is the only connection the app makes on its own.", "Consulta o GitHub ao buscar atualizações. É a única conexão que o app faz por conta própria.", "Consulte GitHub lors de la recherche. C'est la seule connexion que l'app fait d'elle-même.", "Fragt beim Suchen GitHub ab. Das ist die einzige Verbindung, die die App selbst aufbaut.", "アップデート確認時に GitHub に問い合わせます。アプリが自分から行う唯一の通信です。", "检查更新时会访问 GitHub。这是应用自行发起的唯一连接。"]),
    ("Frecuencia", ["Frequency", "Frequência", "Fréquence", "Häufigkeit", "頻度", "频率"]),
    ("Cada cuánto se repite la búsqueda.", ["How often the search repeats.", "A cada quanto tempo a busca se repete.", "Fréquence de la recherche.", "Wie oft die Suche wiederholt wird.", "検索を繰り返す間隔です。", "多久重复检查一次。"]),
    ("Se abre en la barra de menú al encender el equipo.", ["Opens in the menu bar when the computer starts.", "Abre na barra de menus ao ligar o computador.", "S'ouvre dans la barre des menus au démarrage de l'ordinateur.", "Öffnet sich beim Start des Computers in der Menüleiste.", "コンピュータの起動時にメニューバーで開きます。", "开机时在菜单栏中打开。"]),
    ("Abrir al iniciar sesión", ["Open at login", "Abrir ao iniciar sessão", "Ouvrir à l'ouverture de session", "Beim Anmelden öffnen", "ログイン時に開く", "登录时打开"]),
    ("Apariencia", ["Appearance", "Aparência", "Apparence", "Erscheinungsbild", "外観", "外观"]),
    ("Claro, oscuro o el del sistema.", ["Light, dark or the system's.", "Claro, escuro ou o do sistema.", "Clair, sombre ou celui du système.", "Hell, dunkel oder wie das System.", "ライト、ダーク、またはシステムに合わせます。", "浅色、深色或跟随系统。"]),
    ("Sistema", ["System", "Sistema", "Système", "System", "システム", "系统"]),
    ("Claro", ["Light", "Claro", "Clair", "Hell", "ライト", "浅色"]),
    ("Oscuro", ["Dark", "Escuro", "Sombre", "Dunkel", "ダーク", "深色"]),
    ("Idioma", ["Language", "Idioma", "Langue", "Sprache", "言語", "语言"]),
    ("Idioma de la interfaz.", ["Interface language.", "Idioma da interface.", "Langue de l'interface.", "Sprache der Oberfläche.", "表示言語です。", "界面语言。"]),
    ("Elige tu idioma", ["Choose your language", "Escolha seu idioma", "Choisissez votre langue", "Wählen Sie Ihre Sprache", "言語を選択してください", "请选择语言"]),
    ("Podrás cambiarlo después en Ajustes.", ["You can change it later in Settings.", "Você pode mudar depois em Ajustes.", "Vous pourrez le changer plus tard dans Réglages.", "Sie können es später in den Einstellungen ändern.", "あとから設定で変更できます。", "之后可在设置中更改。"]),
    ("Permisos", ["Permissions", "Permissões", "Autorisations", "Berechtigungen", "権限", "权限"]),
    ("Gestión de apps, Homebrew y mas: lo que necesita para actualizar.", ["App Management, Homebrew and mas: what it needs to update.", "Gerenciamento de apps, Homebrew e mas: o que ele precisa para atualizar.", "Gestion des apps, Homebrew et mas : ce qu'il lui faut pour mettre à jour.", "App-Verwaltung, Homebrew und mas: was zum Aktualisieren nötig ist.", "アプリの管理、Homebrew、mas: 更新に必要なものです。", "应用管理、Homebrew 和 mas：更新所需的内容。"]),
    ("Revisar", ["Review", "Revisar", "Vérifier", "Prüfen", "確認", "查看"]),
    ("La app se reiniciará sola al terminar", ["The app will restart by itself when it finishes", "O app reinicia sozinho ao terminar", "L'app redémarrera toute seule à la fin", "Die App startet nach dem Abschluss selbst neu", "完了するとアプリは自動で再起動します", "完成后应用会自动重启"]),
    ("Hay una versión nueva: {0}", ["There is a new version: {0}", "Há uma nova versão: {0}", "Une nouvelle version existe : {0}", "Es gibt eine neue Version: {0}", "新しいバージョンがあります: {0}", "有新版本：{0}"]),
    ("Ruta que usa para buscar y actualizar.", ["Path it uses to search and update.", "Caminho que usa para buscar e atualizar.", "Chemin utilisé pour chercher et mettre à jour.", "Pfad, der zum Suchen und Aktualisieren genutzt wird.", "検索と更新に使うパスです。", "用于查找和更新的路径。"]),
    ("no instalado", ["not installed", "não instalado", "non installé", "nicht installiert", "未インストール", "未安装"]),
    ("Todas tus apps al día, desde una sola cola. Gratis y de código abierto.", ["All your apps up to date, from a single queue. Free and open source.", "Todos os seus apps atualizados, em uma só fila. Grátis e de código aberto.", "Toutes vos apps à jour, depuis une seule file. Gratuit et open source.", "Alle Ihre Apps aktuell, aus einer einzigen Warteschlange. Kostenlos und Open Source.", "すべてのアプリを1つのキューで最新に。無料のオープンソースです。", "所有应用保持最新，只需一个队列。免费且开源。"]),
    ("Autor", ["Author", "Autor", "Auteur", "Autor", "作者", "作者"]),
    ("Licencia", ["License", "Licença", "Licence", "Lizenz", "ライセンス", "许可证"]),
    ("MIT — gratis para usar, copiar y modificar", ["MIT — free to use, copy and modify", "MIT — grátis para usar, copiar e modificar", "MIT — libre d'utilisation, de copie et de modification", "MIT — frei nutzbar, kopierbar und veränderbar", "MIT — 無料で使用・複製・改変できます", "MIT — 可免费使用、复制和修改"]),
    ("Código fuente", ["Source code", "Código-fonte", "Code source", "Quellcode", "ソースコード", "源代码"]),
    // Model / labels
    ("Directa", ["Direct", "Direta", "Directe", "Direkt", "直接", "直接安装"]),
    ("hace {0} d", ["{0} d ago", "há {0} d", "il y a {0} j", "vor {0} T.", "{0}日前", "{0} 天前"]),
    ("hace {0} sem", ["{0} wk ago", "há {0} sem", "il y a {0} sem.", "vor {0} Wo.", "{0}週間前", "{0} 周前"]),
    ("hace {0} m", ["{0} mo ago", "há {0} m", "il y a {0} mois", "vor {0} Mon.", "{0}か月前", "{0} 个月前"]),
    ("hace {0} a", ["{0} y ago", "há {0} a", "il y a {0} an(s)", "vor {0} J.", "{0}年前", "{0} 年前"]),
    ("hoy", ["today", "hoje", "aujourd'hui", "heute", "今日", "今天"]),
    // Catalog categories
    ("Navegadores", ["Browsers", "Navegadores", "Navigateurs", "Browser", "ブラウザ", "浏览器"]),
    ("Desarrollo", ["Development", "Desenvolvimento", "Développement", "Entwicklung", "開発", "开发"]),
    ("Utilidades", ["Utilities", "Utilitários", "Utilitaires", "Dienstprogramme", "ユーティリティ", "实用工具"]),
    ("Comunicación", ["Communication", "Comunicação", "Communication", "Kommunikation", "コミュニケーション", "通讯"]),
    ("Multimedia", ["Media", "Multimídia", "Multimédia", "Multimedia", "メディア", "多媒体"]),
    ("Productividad", ["Productivity", "Produtividade", "Productivité", "Produktivität", "仕事効率化", "效率"]),
    ("Juegos", ["Games", "Jogos", "Jeux", "Spiele", "ゲーム", "游戏"]),
    // Tray and notifications
    ("Abrir Updateeer", ["Open Updateeer", "Abrir o Updateeer", "Ouvrir Updateeer", "Updateeer öffnen", "Updateeer を開く", "打开 Updateeer"]),
    ("Salir de Updateeer", ["Quit Updateeer", "Sair do Updateeer", "Quitter Updateeer", "Updateeer beenden", "Updateeer を終了", "退出 Updateeer"]),
    ("1 actualización disponible", ["1 update available", "1 atualização disponível", "1 mise à jour disponible", "1 Update verfügbar", "アップデート 1 件", "有 1 个可用更新"]),
    ("{0} actualizaciones disponibles", ["{0} updates available", "{0} atualizações disponíveis", "{0} mises à jour disponibles", "{0} Updates verfügbar", "アップデート {0} 件", "有 {0} 个可用更新"]),
    ("Actualizar ahora", ["Update now", "Atualizar agora", "Mettre à jour maintenant", "Jetzt aktualisieren", "今すぐ更新", "立即更新"]),
    ("Más tarde", ["Later", "Mais tarde", "Plus tard", "Später", "あとで", "稍后"]),
    ("Actualizar todas", ["Update all", "Atualizar todos", "Tout mettre à jour", "Alle aktualisieren", "すべて更新", "全部更新"]),
    ("{0} tiene una actualización", ["{0} has an update", "{0} tem uma atualização", "{0} a une mise à jour", "{0} hat ein Update", "{0} のアップデートがあります", "{0} 有可用更新"]),
    ("Está abierta ahora. Versión {0} → {1}.", ["It is open now. Version {0} → {1}.", "Está aberto agora. Versão {0} → {1}.", "Elle est ouverte. Version {0} → {1}.", "Sie ist gerade geöffnet. Version {0} → {1}.", "現在開いています。バージョン {0} → {1}。", "当前已打开。版本 {0} → {1}。"]),
    ("{0} apps abiertas tienen actualizaciones", ["{0} open apps have updates", "{0} apps abertos têm atualizações", "{0} apps ouvertes ont des mises à jour", "{0} geöffnete Apps haben Updates", "開いている {0} 個のアプリにアップデートがあります", "{0} 个已打开的应用有更新"]),
    ("{0}: están abiertas ahora.", ["{0}: they are open now.", "{0}: estão abertos agora.", "{0} : elles sont ouvertes.", "{0}: sie sind gerade geöffnet.", "{0}: 現在開いています。", "{0}：当前已打开。"]),
    ("{0} y {1}", ["{0} and {1}", "{0} e {1}", "{0} et {1}", "{0} und {1}", "{0}、{1}", "{0}和{1}"]),
    ("{0} y {1} más", ["{0} and {1} more", "{0} e mais {1}", "{0} et {1} de plus", "{0} und {1} weitere", "{0} ほか {1} 件", "{0} 等另外 {1} 个"]),
    ("¿Actualizar ahora?", ["Update now?", "Atualizar agora?", "Mettre à jour maintenant ?", "Jetzt aktualisieren?", "今すぐ更新しますか？", "立即更新吗？"]),
    ("Escribe al menos 2 letras", ["Type at least 2 letters", "Digite pelo menos 2 letras", "Saisissez au moins 2 lettres", "Geben Sie mindestens 2 Buchstaben ein", "2文字以上入力してください", "请至少输入 2 个字符"]),
    ("Homebrew · app", ["Homebrew · app", "Homebrew · app", "Homebrew · app", "Homebrew · App", "Homebrew · アプリ", "Homebrew · 应用"]),
    ("Homebrew · herramienta", ["Homebrew · tool", "Homebrew · ferramenta", "Homebrew · outil", "Homebrew · Werkzeug", "Homebrew · ツール", "Homebrew · 工具"]),
    ("{0} d", ["{0} d", "{0} d", "{0} j", "{0} T.", "{0}日", "{0}天"]),
    ("{0} sem", ["{0} wk", "{0} sem", "{0} sem.", "{0} Wo.", "{0}週", "{0}周"]),
    ("{0} m", ["{0} mo", "{0} m", "{0} mois", "{0} Mon.", "{0}か月", "{0}个月"]),
    ("{0} a", ["{0} y", "{0} a", "{0} an(s)", "{0} J.", "{0}年", "{0}年"]),
    ("winget no está instalado", ["winget is not installed", "winget não está instalado", "winget n'est pas installé", "winget ist nicht installiert", "winget がインストールされていません", "未安装 winget"]),
    ("No se pudo abrir Terminal", ["Could not open Terminal", "Não foi possível abrir o Terminal", "Impossible d'ouvrir le Terminal", "Terminal konnte nicht geöffnet werden", "ターミナルを開けませんでした", "无法打开“终端”"]),
    ("mas no está instalado (brew install mas)", ["mas is not installed (brew install mas)", "mas não está instalado (brew install mas)", "mas n'est pas installé (brew install mas)", "mas ist nicht installiert (brew install mas)", "mas がインストールされていません (brew install mas)", "未安装 mas（brew install mas）"]),
    ("Cancelaste la petición de contraseña", ["You cancelled the password request", "Você cancelou o pedido de senha", "Vous avez annulé la demande de mot de passe", "Sie haben die Passwortabfrage abgebrochen", "パスワードの入力をキャンセルしました", "你取消了密码请求"]),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(s: &str) -> Vec<String> {
        let mut v: Vec<String> = s.match_indices('{').filter_map(|(i, _)| s[i..].find('}').map(|j| s[i..=i + j].to_string())).collect();
        v.sort();
        v
    }

    #[test]
    fn every_translation_is_complete_and_keeps_its_placeholders() {
        let mut seen = std::collections::HashSet::new();
        for (es, row) in TABLE {
            assert!(seen.insert(*es), "duplicate key {es}");
            for (i, text) in row.iter().enumerate() {
                assert!(!text.is_empty(), "empty translation {i} for {es}");
                assert_eq!(placeholders(text), placeholders(es), "placeholders differ in column {i} for {es}");
            }
        }
    }

    #[test]
    fn falls_back_and_substitutes() {
        set(Lang::En);
        assert_eq!(t("Ajustes"), "Settings");
        assert_eq!(t("texto que no existe"), "texto que no existe");
        assert_eq!(tf("Actualizadas {0} de {1}", &["2".into(), "5".into()]), "Updated 2 of 5");
        set(Lang::Zh);
        assert_eq!(t("Ajustes"), "设置");
        set(Lang::Es);
        assert_eq!(t("Ajustes"), "Ajustes");
        assert_eq!(Lang::from_code("pt_BR.UTF-8"), Some(Lang::Pt));
        assert_eq!(Lang::from_code("C"), None);
    }
}
