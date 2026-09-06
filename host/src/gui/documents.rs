//! docs配下のMarkdownを列挙し、相対リンクを同じビューアで開く。
use super::*;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::path::{Path, PathBuf};

struct Document {
    path: PathBuf,
    title: String,
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
        entries.sort_by_key(|entry| {
            (
                entry
                    .path
                    .file_name()
                    .is_none_or(|name| name != "wiring.md"),
                entry.title.clone(),
            )
        });
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
        if std::mem::take(&mut self.scroll_to_top) {
            ui.scroll_to_rect(
                egui::Rect::from_min_size(ui.cursor().min, egui::Vec2::ZERO),
                Some(egui::Align::TOP),
            );
        }
        section(
            ui,
            "ドキュメント",
            "配線・操作手順・機体仕様を参照できます。",
        );
        let mut next = None;
        ui.horizontal(|ui| {
            let title = self
                .entries
                .iter()
                .find(|entry| Some(&entry.path) == self.selected.as_ref())
                .map(|entry| entry.title.as_str())
                .unwrap_or("文書を選択");
            egui::ComboBox::from_id_salt("document-picker")
                .selected_text(title)
                .width(420.0)
                .show_ui(ui, |ui| {
                    for entry in &self.entries {
                        if ui
                            .selectable_label(
                                Some(&entry.path) == self.selected.as_ref(),
                                &entry.title,
                            )
                            .on_hover_text(entry.path.display().to_string())
                            .clicked()
                        {
                            next = Some(entry.path.clone());
                        }
                    }
                });
            if ui.button("一覧・本文を再読込").clicked() {
                self.refresh();
            }
            ui.label(
                RichText::new(format!("{} 文書", self.entries.len()))
                    .size(12.0)
                    .color(MUTED),
            );
        });
        if let Some(path) = next {
            self.open(path);
        }
        if !self.error.is_empty() {
            ui.colored_label(DANGER, &self.error);
        }
        ui.add_space(12.0);
        if let Some(path) = self.selected.clone() {
            let prefix = format!("file://{}/", path.parent().unwrap_or(&self.root).display());
            panel().show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::horizontal()
                    .id_salt(("document-width", &path))
                    .show(ui, |ui| {
                        CommonMarkViewer::new()
                            .default_implicit_uri_scheme(prefix)
                            .enable_scroll_to_heading(true)
                            .max_image_width(Some(ui.available_width().max(100.0) as usize))
                            .show(ui, &mut self.cache, &self.source);
                    });
            });
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
            entries.push(Document { path, title });
        }
    }
    Ok(())
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
}
