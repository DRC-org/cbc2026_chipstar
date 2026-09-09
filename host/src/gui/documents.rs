//! docs配下のMarkdownを列挙し、相対リンクを同じビューアで開く。
use super::*;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::path::{Path, PathBuf};

struct Document {
    path: PathBuf,
    title: String,
    category: &'static str,
}
pub(super) struct Documents {
    root: PathBuf,
    entries: Vec<Document>,
    selected: Option<PathBuf>,
    source: String,
    error: String,
    cache: CommonMarkCache,
    links: Vec<(String, PathBuf)>,
    scroll_to_top: bool,
    query: String,
}
impl Documents {
    pub fn new() -> Self {
        let mut documents = Self {
            root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs"),
            entries: Vec::new(),
            selected: None,
            source: String::new(),
            error: String::new(),
            cache: CommonMarkCache::default(),
            links: Vec::new(),
            scroll_to_top: false,
            query: String::new(),
        };
        documents.refresh();
        documents
    }
    fn refresh(&mut self) {
        let mut entries = Vec::new();
        if let Err(error) = collect(&self.root, &mut entries) {
            self.error = error.to_string();
            return;
        }
        entries.sort_by_key(|entry| (category_order(entry.category), entry.title.clone()));
        self.entries = entries;
        let path = self
            .selected
            .clone()
            .filter(|path| path.exists())
            .or_else(|| self.entries.first().map(|entry| entry.path.clone()));
        if let Some(path) = path {
            self.open(path);
        }
    }
    fn open(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path) {
            Ok(source) => {
                self.links = markdown_links(&path, &source);
                self.cache = CommonMarkCache::default();
                for (link, _) in &self.links {
                    self.cache.add_link_hook(link);
                }
                self.source = source;
                self.selected = Some(path);
                self.error.clear();
                self.scroll_to_top = true;
            }
            Err(error) => self.error = format!("文書を読めません: {} — {error}", path.display()),
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        section(
            ui,
            "機体の文書を読む",
            "左の一覧から、配線・操作手順・機体仕様・開発資料を選びます。",
        );
        let mut next = None;
        if !self.error.is_empty() {
            ui.colored_label(DANGER, &self.error);
        }
        ui.add_space(8.0);
        let available_height = ui.available_height().max(360.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(235.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("文書一覧").strong());
                    if ui.small_button("再読込").clicked() {
                        self.refresh();
                    }
                });
                ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("タイトルを検索")
                        .desired_width(225.0),
                );
                let query = self.query.trim().to_lowercase();
                let visible = self
                    .entries
                    .iter()
                    .filter(|entry| document_matches(entry, &self.root, &query))
                    .count();
                ui.label(
                    RichText::new(format!("{visible} / {}件", self.entries.len()))
                        .size(11.0)
                        .color(MUTED),
                );
                egui::ScrollArea::vertical()
                    .id_salt("document-list")
                    .max_height((available_height - 80.0).max(240.0))
                    .show(ui, |ui| {
                        for category in CATEGORIES {
                            let entries = self.entries.iter().filter(|entry| {
                                entry.category == category
                                    && document_matches(entry, &self.root, &query)
                            });
                            let mut any = false;
                            for entry in entries {
                                if !any {
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(category).size(12.0).color(MUTED).strong(),
                                    );
                                    any = true;
                                }
                                if ui
                                    .selectable_label(
                                        Some(&entry.path) == self.selected.as_ref(),
                                        &entry.title,
                                    )
                                    .on_hover_text(relative_path(&self.root, &entry.path))
                                    .clicked()
                                {
                                    next = Some(entry.path.clone());
                                }
                            }
                        }
                        if visible == 0 {
                            ui.label(RichText::new("該当する文書がありません").color(MUTED));
                        }
                    });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                if let Some(path) = self.selected.clone() {
                    let title = self
                        .entries
                        .iter()
                        .find(|entry| entry.path == path)
                        .map(|entry| entry.title.clone())
                        .unwrap_or_else(|| "文書".into());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(title).size(17.0).strong());
                        ui.label(
                            RichText::new(relative_path(&self.root, &path))
                                .size(11.0)
                                .color(MUTED),
                        );
                    });
                    let prefix =
                        format!("file://{}/", path.parent().unwrap_or(&self.root).display());
                    let mut body = egui::ScrollArea::vertical()
                        .id_salt(("document-body", &path))
                        .max_height((available_height - 32.0).max(300.0));
                    if std::mem::take(&mut self.scroll_to_top) {
                        body = body.vertical_scroll_offset(0.0);
                    }
                    panel().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        body.show(ui, |ui| {
                            egui::ScrollArea::horizontal()
                                .id_salt(("document-width", &path))
                                .show(ui, |ui| {
                                    CommonMarkViewer::new()
                                        .default_implicit_uri_scheme(prefix)
                                        .enable_scroll_to_heading(true)
                                        .max_image_width(Some(
                                            ui.available_width().max(100.0) as usize
                                        ))
                                        .show(ui, &mut self.cache, &self.source);
                                });
                        });
                    });
                } else {
                    ui.label(RichText::new("読む文書を選択してください").color(MUTED));
                }
            });
        });
        if let Some(path) = next {
            self.open(path);
        }
        if self.selected.is_some() {
            let clicked = self
                .links
                .iter()
                .find(|(link, _)| self.cache.get_link_hook(link) == Some(true))
                .map(|(_, path)| path.clone());
            if let Some(path) = clicked {
                self.open(path);
            }
        }
    }
}
fn collect(root: &Path, entries: &mut Vec<Document>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect(&entry.path(), entries)?;
        } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "md") {
            let path = entry.path();
            let source = std::fs::read_to_string(&path)?;
            let title = source
                .lines()
                .find_map(|line| line.strip_prefix("# "))
                .map(str::to_owned)
                .unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());
            let category = document_category(root, &path);
            entries.push(Document {
                path,
                title,
                category,
            });
        }
    }
    Ok(())
}

const CATEGORIES: [&str; 5] = [
    "機体・操作",
    "配線・立上げ",
    "基板・通信",
    "調整・調査",
    "開発資料",
];

fn document_category(root: &Path, path: &Path) -> &'static str {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let name = relative
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if path
        .components()
        .any(|component| component.as_os_str() == "investigations")
        || name == "pid_tuning.md"
    {
        "調整・調査"
    } else if matches!(
        name,
        "wiring.md" | "bringup.md" | "firmware_tests.md" | "status_led.md" | "sts3215_bringup.md"
    ) {
        "配線・立上げ"
    } else if name.starts_with("board_")
        || matches!(
            name,
            "device_protocol.md" | "cctl_can_bus.md" | "motor_protocols.md" | "sts_management.md"
        )
    {
        "基板・通信"
    } else if matches!(
        name,
        "ee_operation.md" | "homing.md" | "host_operation.md" | "rtheta_z_machine.md" | "pick_sequences.md"
    ) {
        "機体・操作"
    } else {
        "開発資料"
    }
}

fn category_order(category: &str) -> usize {
    CATEGORIES
        .iter()
        .position(|candidate| *candidate == category)
        .unwrap_or(CATEGORIES.len())
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn document_matches(entry: &Document, root: &Path, query: &str) -> bool {
    query.is_empty()
        || entry.title.to_lowercase().contains(query)
        || relative_path(root, &entry.path)
            .to_lowercase()
            .contains(query)
}
fn markdown_links(path: &Path, source: &str) -> Vec<(String, PathBuf)> {
    use pulldown_cmark::{Event, Parser, Tag};
    Parser::new(source)
        .filter_map(|event| {
            let Event::Start(Tag::Link { dest_url, .. }) = event else {
                return None;
            };
            let file = dest_url.split('#').next()?;
            if file.is_empty() || file.contains("://") || !file.ends_with(".md") {
                return None;
            }
            Some((dest_url.to_string(), path.parent()?.join(file)))
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_markdown_links_are_routed_to_the_document_viewer() {
        let links = markdown_links(
            Path::new("/docs/guide.md"),
            "[local](board.md#pins) [web](https://example.org/a.md) ![img](a.png) ` [code](bad.md) `",
        );
        assert_eq!(
            links,
            vec![("board.md#pins".into(), PathBuf::from("/docs/board.md"))]
        );
    }

    #[test]
    fn documents_are_grouped_by_purpose() {
        let root = Path::new("/docs");
        assert_eq!(
            document_category(root, Path::new("/docs/wiring.md")),
            "配線・立上げ"
        );
        assert_eq!(
            document_category(root, Path::new("/docs/ee_operation.md")),
            "機体・操作"
        );
        assert_eq!(
            document_category(root, Path::new("/docs/board_cctl.md")),
            "基板・通信"
        );
        assert_eq!(
            document_category(root, Path::new("/docs/investigations/z.md")),
            "調整・調査"
        );
    }
}
