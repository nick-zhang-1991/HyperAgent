//! Internationalization — 20 languages across CLI, Web, and Desktop.
//!
//! Set HYPER_LANG or LANG environment variable to any supported locale.
//! Supported: en, zh-CN, es, ar, pt, id, fr, ja, de, ru, ko, vi, it, tr, pl, uk, nl, th, bn, hi

use std::collections::HashMap;
use std::sync::OnceLock;

/// Supported locale
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    En,
    ZhCN,
    Es,
    Ar,
    Pt,
    Id,
    Fr,
    Ja,
    De,
    Ru,
    Ko,
    Vi,
    It,
    Tr,
    Pl,
    Uk,
    Nl,
    Th,
    Bn,
    Hi,
}

impl Locale {
    /// Auto-detect from environment (HYPER_LANG or LANG)
    pub fn detect() -> Self {
        let lang = std::env::var("HYPER_LANG")
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default()
            .to_lowercase();

        // Full locale codes
        if lang.starts_with("zh-cn") || lang.starts_with("zh_cn") || lang.starts_with("zh-hans") { return Locale::ZhCN; }
        if lang.starts_with("es") { return Locale::Es; }
        if lang.starts_with("ar") { return Locale::Ar; }
        if lang.starts_with("pt") { return Locale::Pt; }
        if lang.starts_with("id") { return Locale::Id; }
        if lang.starts_with("fr") { return Locale::Fr; }
        if lang.starts_with("ja") { return Locale::Ja; }
        if lang.starts_with("de") { return Locale::De; }
        if lang.starts_with("ru") { return Locale::Ru; }
        if lang.starts_with("ko") { return Locale::Ko; }
        if lang.starts_with("vi") { return Locale::Vi; }
        if lang.starts_with("it") { return Locale::It; }
        if lang.starts_with("tr") { return Locale::Tr; }
        if lang.starts_with("pl") { return Locale::Pl; }
        if lang.starts_with("uk") { return Locale::Uk; }
        if lang.starts_with("nl") { return Locale::Nl; }
        if lang.starts_with("th") { return Locale::Th; }
        if lang.starts_with("bn") { return Locale::Bn; }
        if lang.starts_with("hi") { return Locale::Hi; }
        Locale::En
    }

    /// Get language code for display
    pub fn code(&self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::ZhCN => "zh-CN",
            Locale::Es => "es",
            Locale::Ar => "ar",
            Locale::Pt => "pt",
            Locale::Id => "id",
            Locale::Fr => "fr",
            Locale::Ja => "ja",
            Locale::De => "de",
            Locale::Ru => "ru",
            Locale::Ko => "ko",
            Locale::Vi => "vi",
            Locale::It => "it",
            Locale::Tr => "tr",
            Locale::Pl => "pl",
            Locale::Uk => "uk",
            Locale::Nl => "nl",
            Locale::Th => "th",
            Locale::Bn => "bn",
            Locale::Hi => "hi",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::ZhCN => "中文(简体)",
            Locale::Es => "Español",
            Locale::Ar => "العربية",
            Locale::Pt => "Português",
            Locale::Id => "Bahasa Indonesia",
            Locale::Fr => "Français",
            Locale::Ja => "日本語",
            Locale::De => "Deutsch",
            Locale::Ru => "Русский",
            Locale::Ko => "한국어",
            Locale::Vi => "Tiếng Việt",
            Locale::It => "Italiano",
            Locale::Tr => "Türkçe",
            Locale::Pl => "Polski",
            Locale::Uk => "Українська",
            Locale::Nl => "Nederlands",
            Locale::Th => "ไทย",
            Locale::Bn => "বাংলা",
            Locale::Hi => "हिन्दी",
        }
    }

    /// List all supported locales
    pub fn all() -> Vec<Locale> {
        vec![
            Locale::En, Locale::ZhCN, Locale::Es, Locale::Ar, Locale::Pt,
            Locale::Id, Locale::Fr, Locale::Ja, Locale::De, Locale::Ru,
            Locale::Ko, Locale::Vi, Locale::It, Locale::Tr, Locale::Pl,
            Locale::Uk, Locale::Nl, Locale::Th, Locale::Bn, Locale::Hi,
        ]
    }
}

/// Global locale storage
static CURRENT_LOCALE: OnceLock<Locale> = OnceLock::new();

/// Initialize i18n with detected locale
pub fn init(locale: Locale) {
    let _ = CURRENT_LOCALE.set(locale);
}

/// Get current locale
pub fn current() -> Locale {
    *CURRENT_LOCALE.get().unwrap_or(&Locale::En)
}

/// Get translations for current locale
fn translations() -> &'static HashMap<&'static str, &'static str> {
    match current() {
        Locale::En => &L10N_EN,
        Locale::ZhCN => &L10N_ZH_CN,
        Locale::Es => &L10N_ES,
        Locale::Ar => &L10N_AR,
        Locale::Pt => &L10N_PT,
        Locale::Id => &L10N_ID,
        Locale::Fr => &L10N_FR,
        Locale::Ja => &L10N_JA,
        Locale::De => &L10N_DE,
        Locale::Ru => &L10N_RU,
        Locale::Ko => &L10N_KO,
        Locale::Vi => &L10N_VI,
        Locale::It => &L10N_IT,
        Locale::Tr => &L10N_TR,
        Locale::Pl => &L10N_PL,
        Locale::Uk => &L10N_UK,
        Locale::Nl => &L10N_NL,
        Locale::Th => &L10N_TH,
        Locale::Bn => &L10N_BN,
        Locale::Hi => &L10N_HI,
    }
}

/// Translate a key. Returns key if not found.
pub fn t(key: &str) -> &str {
    translations().get(key).copied().unwrap_or(key)
}

/// Translate with arguments (simple {} replacement)
pub fn t_with(key: &str, args: &[&str]) -> String {
    let tmpl = t(key);
    let mut result = tmpl.to_string();
    for arg in args {
        if let Some(pos) = result.find("{}") {
            result.replace_range(pos..pos + 2, arg);
        }
    }
    result
}

static L10N_EN: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_ZH_CN: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_ES: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_AR: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_PT: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_ID: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_FR: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_JA: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_DE: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_RU: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_KO: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_VI: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_IT: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_TR: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_PL: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_UK: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_NL: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_TH: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_BN: OnceLock<HashMap<&str, &str>> = OnceLock::new();

static L10N_HI: OnceLock<HashMap<&str, &str>> = OnceLock::new();

fn init_L10N_EN() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — General-Purpose AI Agent");
    m.insert("init_start", "Initializing HyperAgent...");
    m.insert("serve_start", "Server started on http://127.0.0.1:{}");
    m.insert("analyze_title", "Code Analysis");
    m.insert("memory_start", "Global Memory");
    m.insert("feedback_good", "Feedback recorded: positive");
    m.insert("feedback_bad", "Correction recorded: will not repeat");
    m.insert("error_llm_provider", "LLM provider error");
    m.insert("error_config_load", "Failed to load configuration");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_ZH_CN() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — 通用 AI Agent");
    m.insert("init_start", "正在初始化 HyperAgent...");
    m.insert("serve_start", "服务器已启动: http://127.0.0.1:{}");
    m.insert("analyze_title", "代码分析");
    m.insert("memory_start", "全局记忆");
    m.insert("feedback_good", "反馈已记录: 正面");
    m.insert("feedback_bad", "纠正已记录: 不会再犯");
    m.insert("error_llm_provider", "LLM 提供方错误");
    m.insert("error_config_load", "配置加载失败");
    m.insert("error_no_project", "未找到项目");
    m.insert("error_network", "网络错误");
    m.insert("error_sandbox", "沙箱执行失败");
    m.insert("error_parse_json", "JSON 解析失败");
    m.insert("skill_installed", "技能已安装: {}");
    m.insert("skill_not_found", "未找到技能");
    m.insert("swarm_start", "启动 {} 个并行 Agent");
    m
}

fn init_L10N_ES() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — Agente de IA de propósito general");
    m.insert("init_start", "Inicializando HyperAgent...");
    m.insert("serve_start", "Servidor iniciado en http://127.0.0.1:{}");
    m.insert("analyze_title", "Análisis de Código");
    m.insert("memory_start", "Memoria Global");
    m.insert("feedback_good", "Comentario registrado: positivo");
    m.insert("feedback_bad", "Corrección registrada: no se repetirá");
    m.insert("error_llm_provider", "Error del proveedor LLM");
    m.insert("error_config_load", "Error al cargar la configuración");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_AR() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — وكيل ذكاء اصطناعي للأغراض العامة");
    m.insert("init_start", "جارٍ تهيئة HyperAgent...");
    m.insert("serve_start", "تم تشغيل الخادم على http://127.0.0.1:{}");
    m.insert("analyze_title", "تحليل الكود");
    m.insert("memory_start", "الذاكرة العالمية");
    m.insert("feedback_good", "تم تسجيل الملاحظات: إيجابي");
    m.insert("feedback_bad", "تم تسجيل التصحيح: لن يتكرر");
    m.insert("error_llm_provider", "خطأ في مزود LLM");
    m.insert("error_config_load", "فشل تحميل التكوين");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_PT() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — Agente de IA de uso geral");
    m.insert("init_start", "Inicializando HyperAgent...");
    m.insert("serve_start", "Servidor iniciado em http://127.0.0.1:{}");
    m.insert("analyze_title", "Análise de Código");
    m.insert("memory_start", "Memória Global");
    m.insert("feedback_good", "Feedback registrado: positivo");
    m.insert("feedback_bad", "Correção registrada: não será repetida");
    m.insert("error_llm_provider", "Erro do provedor LLM");
    m.insert("error_config_load", "Falha ao carregar configuração");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_ID() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — Agen AI Serbaguna");
    m.insert("init_start", "Menginisialisasi HyperAgent...");
    m.insert("serve_start", "Server berjalan di http://127.0.0.1:{}");
    m.insert("analyze_title", "Analisis Kode");
    m.insert("memory_start", "Memori Global");
    m.insert("feedback_good", "Umpan balik dicatat: positif");
    m.insert("feedback_bad", "Koreksi dicatat: tidak akan diulangi");
    m.insert("error_llm_provider", "Kesalahan penyedia LLM");
    m.insert("error_config_load", "Gagal memuat konfigurasi");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_FR() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — Agent IA polyvalent");
    m.insert("init_start", "Initialisation de HyperAgent...");
    m.insert("serve_start", "Serveur démarré sur http://127.0.0.1:{}");
    m.insert("analyze_title", "Analyse de Code");
    m.insert("memory_start", "Mémoire Globale");
    m.insert("feedback_good", "Commentaire enregistré : positif");
    m.insert("feedback_bad", "Correction enregistrée : ne se répétera pas");
    m.insert("error_llm_provider", "Erreur du fournisseur LLM");
    m.insert("error_config_load", "Échec du chargement de la configuration");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_JA() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — 汎用AIエージェント");
    m.insert("init_start", "HyperAgentを初期化中...");
    m.insert("serve_start", "サーバー起動: http://127.0.0.1:{}");
    m.insert("analyze_title", "コード分析");
    m.insert("memory_start", "グローバルメモリ");
    m.insert("feedback_good", "フィードバック記録: 肯定的");
    m.insert("feedback_bad", "修正を記録: 繰り返しません");
    m.insert("error_llm_provider", "LLMプロバイダーエラー");
    m.insert("error_config_load", "設定の読み込みに失敗");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_DE() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — Allzweck-KI-Agent");
    m.insert("init_start", "HyperAgent wird initialisiert...");
    m.insert("serve_start", "Server gestartet auf http://127.0.0.1:{}");
    m.insert("analyze_title", "Code-Analyse");
    m.insert("memory_start", "Globaler Speicher");
    m.insert("feedback_good", "Feedback aufgezeichnet: positiv");
    m.insert("feedback_bad", "Korrektur aufgezeichnet: wird nicht wiederholt");
    m.insert("error_llm_provider", "LLM-Anbieterfehler");
    m.insert("error_config_load", "Konfiguration konnte nicht geladen werden");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_RU() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — Универсальный ИИ-агент");
    m.insert("init_start", "Инициализация HyperAgent...");
    m.insert("serve_start", "Сервер запущен на http://127.0.0.1:{}");
    m.insert("analyze_title", "Анализ кода");
    m.insert("memory_start", "Глобальная память");
    m.insert("feedback_good", "Отзыв записан: положительный");
    m.insert("feedback_bad", "Исправление записано: не повторится");
    m.insert("error_llm_provider", "Ошибка провайдера LLM");
    m.insert("error_config_load", "Не удалось загрузить конфигурацию");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_KO() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — 범용 AI 에이전트");
    m.insert("init_start", "HyperAgent 초기화 중...");
    m.insert("serve_start", "서버 시작됨: http://127.0.0.1:{}");
    m.insert("analyze_title", "코드 분석");
    m.insert("memory_start", "글로벌 메모리");
    m.insert("feedback_good", "피드백 기록됨: 긍정적");
    m.insert("feedback_bad", "수정 기록됨: 반복하지 않음");
    m.insert("error_llm_provider", "LLM 제공자 오류");
    m.insert("error_config_load", "구성 로드 실패");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_VI() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — Tác nhân AI đa năng");
    m.insert("init_start", "Đang khởi tạo HyperAgent...");
    m.insert("serve_start", "Máy chủ đã khởi động tại http://127.0.0.1:{}");
    m.insert("analyze_title", "Phân tích Mã");
    m.insert("memory_start", "Bộ nhớ Toàn cục");
    m.insert("feedback_good", "Phản hồi đã ghi: tích cực");
    m.insert("feedback_bad", "Sửa lỗi đã ghi: sẽ không lặp lại");
    m.insert("error_llm_provider", "Lỗi nhà cung cấp LLM");
    m.insert("error_config_load", "Không tải được cấu hình");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_IT() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — Agente AI generico");
    m.insert("init_start", "Inizializzazione di HyperAgent...");
    m.insert("serve_start", "Server avviato su http://127.0.0.1:{}");
    m.insert("analyze_title", "Analisi del Codice");
    m.insert("memory_start", "Memoria Globale");
    m.insert("feedback_good", "Feedback registrato: positivo");
    m.insert("feedback_bad", "Correzione registrata: non sarà ripetuta");
    m.insert("error_llm_provider", "Errore del provider LLM");
    m.insert("error_config_load", "Caricamento configurazione fallito");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_TR() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — Genel Amaçlı Yapay Zeka Ajanı");
    m.insert("init_start", "HyperAgent başlatılıyor...");
    m.insert("serve_start", "Sunucu başlatıldı: http://127.0.0.1:{}");
    m.insert("analyze_title", "Kod Analizi");
    m.insert("memory_start", "Küresel Bellek");
    m.insert("feedback_good", "Geri bildirim kaydedildi: olumlu");
    m.insert("feedback_bad", "Düzeltme kaydedildi: tekrarlanmayacak");
    m.insert("error_llm_provider", "LLM sağlayıcı hatası");
    m.insert("error_config_load", "Yapılandırma yüklenemedi");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_PL() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — Uniwersalny agent AI");
    m.insert("init_start", "Inicjalizacja HyperAgent...");
    m.insert("serve_start", "Serwer uruchomiony na http://127.0.0.1:{}");
    m.insert("analyze_title", "Analiza Kodu");
    m.insert("memory_start", "Pamięć Globalna");
    m.insert("feedback_good", "Opinia zapisana: pozytywna");
    m.insert("feedback_bad", "Korekta zapisana: nie powtórzy się");
    m.insert("error_llm_provider", "Błąd dostawcy LLM");
    m.insert("error_config_load", "Nie udało się załadować konfiguracji");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_UK() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — Універсальний ШІ-агент");
    m.insert("init_start", "Ініціалізація HyperAgent...");
    m.insert("serve_start", "Сервер запущено на http://127.0.0.1:{}");
    m.insert("analyze_title", "Аналіз коду");
    m.insert("memory_start", "Глобальна пам'ять");
    m.insert("feedback_good", "Відгук записано: позитивний");
    m.insert("feedback_bad", "Виправлення записано: не повториться");
    m.insert("error_llm_provider", "Помилка провайдера LLM");
    m.insert("error_config_load", "Не вдалося завантажити конфігурацію");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_NL() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — AI-agent voor algemeen gebruik");
    m.insert("init_start", "HyperAgent initialiseren...");
    m.insert("serve_start", "Server gestart op http://127.0.0.1:{}");
    m.insert("analyze_title", "Code-analyse");
    m.insert("memory_start", "Globaal Geheugen");
    m.insert("feedback_good", "Feedback vastgelegd: positief");
    m.insert("feedback_bad", "Correctie vastgelegd: wordt niet herhaald");
    m.insert("error_llm_provider", "LLM-provider fout");
    m.insert("error_config_load", "Configuratie laden mislukt");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_TH() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — เอเจนต์ AI อเนกประสงค์");
    m.insert("init_start", "กำลังเริ่มต้น HyperAgent...");
    m.insert("serve_start", "เซิร์ฟเวอร์เริ่มทำงานที่ http://127.0.0.1:{}");
    m.insert("analyze_title", "การวิเคราะห์โค้ด");
    m.insert("memory_start", "หน่วยความจำส่วนกลาง");
    m.insert("feedback_good", "บันทึกข้อเสนอแนะ: เชิงบวก");
    m.insert("feedback_bad", "บันทึกการแก้ไข: จะไม่ทำซ้ำ");
    m.insert("error_llm_provider", "ข้อผิดพลาดผู้ให้บริการ LLM");
    m.insert("error_config_load", "โหลดการกำหนดค่าล้มเหลว");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_BN() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — সাধারণ উদ্দেশ্যের AI এজেন্ট");
    m.insert("init_start", "HyperAgent শুরু করা হচ্ছে...");
    m.insert("serve_start", "সার্ভার শুরু হয়েছে http://127.0.0.1:{}");
    m.insert("analyze_title", "কোড বিশ্লেষণ");
    m.insert("memory_start", "গ্লোবাল মেমোরি");
    m.insert("feedback_good", "প্রতিক্রিয়া রেকর্ড করা হয়েছে: ইতিবাচক");
    m.insert("feedback_bad", "সংশোধন রেকর্ড করা হয়েছে: পুনরাবৃত্তি হবে না");
    m.insert("error_llm_provider", "LLM প্রদানকারী ত্রুটি");
    m.insert("error_config_load", "কনফিগারেশন লোড করতে ব্যর্থ");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

fn init_L10N_HI() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("welcome_banner", "HyperAgent — सामान्य प्रयोजन AI एजेंट");
    m.insert("init_start", "HyperAgent प्रारंभ हो रहा है...");
    m.insert("serve_start", "सर्वर शुरू: http://127.0.0.1:{}");
    m.insert("analyze_title", "कोड विश्लेषण");
    m.insert("memory_start", "वैश्विक स्मृति");
    m.insert("feedback_good", "प्रतिक्रिया दर्ज: सकारात्मक");
    m.insert("feedback_bad", "सुधार दर्ज: दोहराया नहीं जाएगा");
    m.insert("error_llm_provider", "LLM प्रदाता त्रुटि");
    m.insert("error_config_load", "कॉन्फ़िगरेशन लोड करने में विफल");
    m.insert("error_no_project", "No project found");
    m.insert("error_network", "Network error");
    m.insert("error_sandbox", "Sandbox execution failed");
    m.insert("error_parse_json", "Failed to parse JSON response");
    m.insert("skill_installed", "Skill installed: {}");
    m.insert("skill_not_found", "No skills found");
    m.insert("swarm_start", "Starting {} parallel agents");
    m
}

