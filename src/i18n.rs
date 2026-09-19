//! User-facing text and language selection.
//!
//! Keep UI copy here so widgets and rendering code do not accumulate their
//! own partially translated strings. `CODE_MAP_LANG` and `--lang=...` are
//! useful for automation; the in-app selector can override the detected value.

use std::env;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    English,
    SimplifiedChinese,
}

impl Language {
    pub fn detect() -> Self {
        let command_line = env::args().find_map(|arg| {
            arg.strip_prefix("--lang=")
                .and_then(Self::from_language_tag)
        });
        if let Some(language) = command_line {
            return language;
        }

        if let Some(language) = env::var("CODE_MAP_LANG")
            .ok()
            .and_then(|tag| Self::from_language_tag(&tag))
        {
            return language;
        }

        for name in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
            if let Some(language) = env::var(name)
                .ok()
                .and_then(|tag| Self::from_language_tag(&tag))
            {
                return language;
            }
        }

        #[cfg(target_os = "windows")]
        if let Some(language) = windows_locale().and_then(|tag| Self::from_language_tag(&tag)) {
            return language;
        }

        Self::English
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::SimplifiedChinese,
            _ => Self::English,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::English => 0,
            Self::SimplifiedChinese => 1,
        }
    }

    fn from_language_tag(tag: &str) -> Option<Self> {
        let tag = tag
            .split([':', '.', '@'])
            .next()
            .unwrap_or(tag)
            .replace('_', "-")
            .to_ascii_lowercase();
        if tag == "zh" || tag.starts_with("zh-") {
            Some(Self::SimplifiedChinese)
        } else if tag == "en" || tag.starts_with("en-") {
            Some(Self::English)
        } else {
            None
        }
    }

    pub fn search_placeholder(self) -> &'static str {
        match self {
            Self::English => "Search files and folders",
            Self::SimplifiedChinese => "搜索文件和文件夹",
        }
    }

    pub fn color_modes(self) -> [&'static str; 3] {
        match self {
            Self::English => ["File type", "Recently changed", "Most changed"],
            Self::SimplifiedChinese => ["文件类型", "最近更改", "更改最多"],
        }
    }

    pub fn detail_levels(self) -> [&'static str; 4] {
        match self {
            Self::English => ["Normal", "High", "Ultra", "Custom"],
            Self::SimplifiedChinese => ["普通", "高", "极致", "自定义"],
        }
    }

    pub fn language_names() -> [&'static str; 2] {
        ["English", "简体中文"]
    }

    pub fn fit(self) -> &'static str {
        match self {
            Self::English => "Fit",
            Self::SimplifiedChinese => "适应窗口",
        }
    }

    pub fn show_ignored(self) -> &'static str {
        match self {
            Self::English => "Show ignored",
            Self::SimplifiedChinese => "显示忽略项",
        }
    }

    pub fn inspector(self) -> &'static str {
        match self {
            Self::English => "INSPECTOR",
            Self::SimplifiedChinese => "检查器",
        }
    }

    pub fn select_hint(self) -> &'static str {
        match self {
            Self::English => "Click something on the map",
            Self::SimplifiedChinese => "点击地图中的项目",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Self::English => "Scroll: zoom\nDrag: pan (2D) or orbit (3D)\nShift or right drag: pan (3D)\nClick: inspect\nDouble click: fly to it\nEnter in search: next match\nStriped boxes are ignored by git.\nClick one to read it.",
            Self::SimplifiedChinese => "滚轮：缩放\n拖动：平移（2D）或环绕（3D）\nShift 或右键拖动：平移（3D）\n单击：检查\n双击：飞至目标\n搜索中按 Enter：下一个匹配项\n条纹方块是 Git 忽略项。\n点击可读取内容。",
        }
    }

    pub fn custom_detail(self) -> &'static str {
        match self {
            Self::English => "CUSTOM DETAIL",
            Self::SimplifiedChinese => "自定义画质",
        }
    }

    pub fn geometry_detail(self) -> &'static str {
        match self {
            Self::English => "Geometry detail",
            Self::SimplifiedChinese => "几何细节",
        }
    }

    pub fn text_detail(self) -> &'static str {
        match self {
            Self::English => "Text detail",
            Self::SimplifiedChinese => "文字细节",
        }
    }

    pub fn render_budget(self) -> &'static str {
        match self {
            Self::English => "Render budget (%)",
            Self::SimplifiedChinese => "渲染预算（%）",
        }
    }

    pub fn scanning(self, path: &str) -> String {
        match self {
            Self::English => format!("Scanning {path} ..."),
            Self::SimplifiedChinese => format!("正在扫描 {path}……"),
        }
    }

    pub fn reading_history(self) -> String {
        match self {
            Self::English => "Reading git history ...".to_string(),
            Self::SimplifiedChinese => "正在读取 Git 历史……".to_string(),
        }
    }

    pub fn search_matches(self, count: &str, query: &str) -> String {
        match self {
            Self::English => {
                format!("{count} matches for \"{query}\". Press Enter to fly to the next one.")
            }
            Self::SimplifiedChinese => {
                format!("“{query}”有 {count} 个匹配项。按 Enter 跳转到下一个。")
            }
        }
    }

    pub fn summary(self, files: &str, lines: &str, commits: Option<&str>) -> String {
        match (self, commits) {
            (Self::English, Some(commits)) => {
                format!("{files} files, {lines} lines, {commits} commits of history")
            }
            (Self::English, None) => format!("{files} files, {lines} lines"),
            (Self::SimplifiedChinese, Some(commits)) => {
                format!("{files} 个文件，{lines} 行，{commits} 个历史提交")
            }
            (Self::SimplifiedChinese, None) => format!("{files} 个文件，{lines} 行"),
        }
    }

    pub fn scanned(self, summary: &str, seconds: f64, used_git: bool) -> String {
        match self {
            Self::English => format!(
                "{summary}, scanned in {seconds:.2}s ({})",
                if used_git {
                    "ignore rules from git"
                } else {
                    "no git: read .gitignore files"
                }
            ),
            Self::SimplifiedChinese => format!(
                "{summary}，扫描耗时 {seconds:.2} 秒（{}）",
                if used_git {
                    "使用 Git 忽略规则"
                } else {
                    "无 Git：读取 .gitignore 文件"
                }
            ),
        }
    }

    pub fn scan_failed(self, error: &str) -> String {
        match self {
            Self::English => format!("Could not scan: {error}"),
            Self::SimplifiedChinese => format!("扫描失败：{error}"),
        }
    }

    pub fn expanded_ignored(self, path: &str, count: &str) -> String {
        match self {
            Self::English => format!("Read ignored {path}: {count} files"),
            Self::SimplifiedChinese => format!("已读取忽略项 {path}：{count} 个文件"),
        }
    }

    pub fn history_failed(self, error: &str) -> String {
        match self {
            Self::English => format!("No git history: {error}"),
            Self::SimplifiedChinese => format!("无法读取 Git 历史：{error}"),
        }
    }

    pub fn no_folder(self) -> &'static str {
        match self {
            Self::English => "No folder opened",
            Self::SimplifiedChinese => "尚未打开文件夹",
        }
    }

    pub fn folder_details(self, files: &str, lines: &str, bytes: &str, children: usize) -> String {
        match self {
            Self::English => {
                format!("Folder\n{files} files\n{lines} lines\n{bytes}\n{children} direct children")
            }
            Self::SimplifiedChinese => {
                format!("文件夹\n{files} 个文件\n{lines} 行\n{bytes}\n{children} 个直接子项")
            }
        }
    }

    pub fn text_file_details(self, lines: &str, comments: &str, bytes: &str) -> String {
        match self {
            Self::English => {
                format!("Text file\n{lines} lines ({comments} comment lines)\n{bytes}")
            }
            Self::SimplifiedChinese => {
                format!("文本文件\n{lines} 行（{comments} 行注释）\n{bytes}")
            }
        }
    }

    pub fn binary_details(self, bytes: &str) -> String {
        match self {
            Self::English => format!("Binary or very large file\n{bytes}"),
            Self::SimplifiedChinese => format!("二进制文件或超大文件\n{bytes}"),
        }
    }

    pub fn ignored_details(self, directory: bool, loading: bool) -> String {
        match self {
            Self::English => format!(
                "Ignored {}\n{}",
                if directory { "folder" } else { "file" },
                if loading {
                    "Reading it now ..."
                } else {
                    "Not read yet. Click it to read it."
                }
            ),
            Self::SimplifiedChinese => format!(
                "已忽略的{}\n{}",
                if directory { "文件夹" } else { "文件" },
                if loading {
                    "正在读取……"
                } else {
                    "尚未读取。点击即可读取。"
                }
            ),
        }
    }

    pub fn git_details(self, commits: &str, age: &str) -> String {
        match self {
            Self::English => format!("\n\nGit: {commits} commits\nlast changed {age}"),
            Self::SimplifiedChinese => format!("\n\nGit：{commits} 个提交\n最后更改于{age}"),
        }
    }

    pub fn no_commits(self) -> &'static str {
        match self {
            Self::English => "\n\nGit: no commits in history",
            Self::SimplifiedChinese => "\n\nGit：历史中没有提交",
        }
    }

    pub fn matched_gitignore(self) -> &'static str {
        match self {
            Self::English => "\n\nMatched by .gitignore",
            Self::SimplifiedChinese => "\n\n匹配 .gitignore 规则",
        }
    }

    pub fn reading_suffix(self) -> &'static str {
        match self {
            Self::English => "reading...",
            Self::SimplifiedChinese => "正在读取……",
        }
    }

    pub fn ignored_suffix(self) -> &'static str {
        match self {
            Self::English => "ignored",
            Self::SimplifiedChinese => "已忽略",
        }
    }

    pub fn bytes(self, bytes: u64) -> String {
        match bytes {
            b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
            b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
            b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / 1024.0),
            b => match self {
                Self::English => format!("{b} bytes"),
                Self::SimplifiedChinese => format!("{b} 字节"),
            },
        }
    }

    pub fn age(self, seconds: i64) -> String {
        let days = seconds / 86_400;
        match self {
            Self::English => match days {
                d if d < 1 => "today".to_string(),
                1 => "yesterday".to_string(),
                d if d < 60 => format!("{d} days ago"),
                d if d < 730 => format!("{} months ago", d / 30),
                d => format!("{} years ago", d / 365),
            },
            Self::SimplifiedChinese => match days {
                d if d < 1 => "今天".to_string(),
                1 => "昨天".to_string(),
                d if d < 60 => format!("{d} 天前"),
                d if d < 730 => format!("{} 个月前", d / 30),
                d => format!("{} 年前", d / 365),
            },
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_locale() -> Option<String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultLocaleName(locale_name: *mut u16, locale_name_len: i32) -> i32;
    }

    let mut buffer = [0u16; 85];
    // SAFETY: Windows receives a valid writable buffer and its exact length.
    let len = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 1 {
        None
    } else {
        String::from_utf16(&buffer[..len as usize - 1]).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::Language;

    #[test]
    fn parses_supported_language_tags() {
        assert_eq!(
            Language::from_language_tag("zh_CN.UTF-8"),
            Some(Language::SimplifiedChinese)
        );
        assert_eq!(
            Language::from_language_tag("zh-Hans-CN"),
            Some(Language::SimplifiedChinese)
        );
        assert_eq!(
            Language::from_language_tag("en-US"),
            Some(Language::English)
        );
        assert_eq!(Language::from_language_tag("de-DE"), None);
    }

    #[test]
    fn custom_detail_captions_have_both_locales() {
        assert_eq!(Language::English.geometry_detail(), "Geometry detail");
        assert_eq!(Language::English.text_detail(), "Text detail");
        assert_eq!(Language::English.render_budget(), "Render budget (%)");
        assert_eq!(Language::SimplifiedChinese.geometry_detail(), "几何细节");
        assert_eq!(Language::SimplifiedChinese.text_detail(), "文字细节");
        assert_eq!(Language::SimplifiedChinese.render_budget(), "渲染预算（%）");
    }

    #[test]
    fn localizes_compound_status_text() {
        assert_eq!(
            Language::English.summary("12", "345", Some("8")),
            "12 files, 345 lines, 8 commits of history"
        );
        assert_eq!(
            Language::SimplifiedChinese.search_matches("3", "地图"),
            "“地图”有 3 个匹配项。按 Enter 跳转到下一个。"
        );
    }
}
