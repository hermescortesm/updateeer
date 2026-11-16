//! Banners above the pages: a new Updateeer release, a missing macOS permission, a missing tool.
use super::*;

impl UpdatesApp {
    /// A newer Updateeer exists: install it (or open the download), with its release notes.
    pub(super) fn release_banner(&mut self, ui: &mut Ui) -> bool {
        let Some(release) = self.self_update.new_release.clone() else { return false };
        let can_install = crate::selfupdate::can_self_install();
        let installing = self.self_update.self_update_rx.is_some();
        let (mut act, mut notes) = (false, false);
        Frame::new()
            .fill(banner_bg())
            .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xFF, 0x8A, 0x2A, 77)))
            .corner_radius(BANNER_CORNER_RADIUS)
            .inner_margin(Margin::symmetric(BANNER_INNER_MARGIN_X, BANNER_INNER_MARGIN_Y))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = SPACING_MD;
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 320.0);
                        ui.spacing_mut().item_spacing.y = SPACING_XS;
                        ui.label(RichText::new(tf1("Updateeer {0} está disponible", &release.version.clone())).size(FONT_SIZE_BODY).family(semibold()));
                        let detail = match (&self.self_update.self_update_error, installing, can_install) {
                            (Some(e), _, _) => tf1("No se pudo actualizar: {0}", &e.to_string()),
                            (_, true, _) => t("La app se reiniciará sola al terminar.").to_string(),
                            (_, _, true) => tf1("Tienes la {0}. Se descarga, se verifica y la app se reinicia sola.", env!("CARGO_PKG_VERSION")),
                            _ => tf1("Tienes la {0}. Descárgala desde la página de la versión.", env!("CARGO_PKG_VERSION")),
                        };
                        ui.label(RichText::new(detail).size(FONT_SIZE_SMALL).color(muted()));
                        if installing && self.self_update.self_update_error.is_none() {
                            ui.add_space(SPACING_XS);
                            self.update_progress(ui);
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if can_install { t("Instalar y reiniciar") } else { t("Descargar") };
                        act = widgets::button(ui, Some(Icon::Download), label, Kind::Primary, BUTTON_HEIGHT_MD, !installing).clicked();
                        notes = widgets::button(ui, None, if self.self_update.show_notes { t("Ocultar novedades") } else { t("Novedades") }, Kind::Soft, BUTTON_HEIGHT_MD, true).clicked();
                    });
                });
                if self.self_update.show_notes {
                    ui.add_space(SPACING_SM);
                    ui.label(RichText::new(crate::selfupdate::summary(&release.notes)).size(FONT_SIZE_SMALL).color(muted()));
                }
            });
        if notes {
            self.self_update.show_notes = !self.self_update.show_notes;
        }
        if act {
            if can_install {
                self.start_self_update();
            } else {
                crate::selfupdate::open(&release.url);
            }
        }
        true
    }

    /// macOS blocks changes to other apps until «App Management» is on: say so before updating.
    pub(super) fn permission_banner(&mut self, ui: &mut Ui) -> bool {
        if !cfg!(target_os = "macos") || self.setup.access != crate::permissions::Access::Denied {
            return false;
        }
        let mut open = false;
        Frame::new()
            .fill(banner_bg())
            .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xFF, 0x8A, 0x2A, 77)))
            .corner_radius(BANNER_CORNER_RADIUS)
            .inner_margin(Margin::symmetric(BANNER_INNER_MARGIN_X, BANNER_INNER_MARGIN_Y))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = SPACING_MD;
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 200.0);
                        ui.spacing_mut().item_spacing.y = SPACING_XS;
                        ui.label(RichText::new(t("Falta el permiso «Gestión de apps»")).size(FONT_SIZE_BODY).family(semibold()));
                        ui.label(RichText::new(t("macOS lo pide para que Updateeer pueda actualizar tus apps. Actívalo ahora en Ajustes del Sistema → Privacidad y seguridad → Gestión de apps.")).size(FONT_SIZE_SMALL).color(muted()));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        open = widgets::button(ui, None, t("Abrir ajustes"), Kind::Primary, BUTTON_HEIGHT_MD, true).clicked();
                    });
                });
            });
        if open {
            crate::permissions::open_settings();
        }
        true
    }

    /// Offers to install Homebrew or `mas` (macOS). Nothing is installed without a click.
    /// Returns whether it drew anything.
    pub(super) fn setup_banner(&mut self, ui: &mut Ui) -> bool {
        let (brew, mas) = self.setup.tools;
        if !cfg!(target_os = "macos") || self.settings.hide_setup || (brew && mas) {
            return false;
        }
        let installing = self.setup.mas_install.is_some();
        let (title, text, action) = if !brew {
            (
                t("Falta Homebrew"),
                t("Updateeer lo usa para buscar y actualizar tus apps. Se instala desde Terminal y te pedirá tu contraseña."),
                t("Instalar Homebrew"),
            )
        } else {
            (
                t("Falta mas para las apps de la App Store"),
                t("Es una herramienta pequeña que se instala con Homebrew. Sin ella no se ven las actualizaciones de la App Store."),
                t("Instalar mas"),
            )
        };
        let mut install = false;
        let mut dismiss = false;
        Frame::new()
            .fill(banner_bg())
            .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xFF, 0x8A, 0x2A, 77)))
            .corner_radius(BANNER_CORNER_RADIUS)
            .inner_margin(Margin::symmetric(BANNER_INNER_MARGIN_X, BANNER_INNER_MARGIN_Y))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = SPACING_MD;
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 300.0);
                        ui.spacing_mut().item_spacing.y = SPACING_XS;
                        ui.label(RichText::new(title).size(FONT_SIZE_BODY).family(semibold()));
                        let detail = self.setup.setup_error.clone().unwrap_or_else(|| text.to_string());
                        ui.label(RichText::new(detail).size(FONT_SIZE_SMALL).color(muted()));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if installing { t("Instalando…") } else { action };
                        install = widgets::button(ui, None, label, Kind::Primary, BUTTON_HEIGHT_MD, !installing).clicked();
                        dismiss = widgets::button(ui, None, t("Ahora no"), Kind::Soft, BUTTON_HEIGHT_MD, !installing).clicked();
                    });
                });
            });
        if dismiss {
            self.settings.hide_setup = true;
            self.settings.save();
        }
        if install {
            self.install_tool(brew);
        }
        true
    }
}
