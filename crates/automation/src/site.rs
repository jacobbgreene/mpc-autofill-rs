// SPDX-License-Identifier: GPL-3.0-only
// Adapts MPC Autofill's desktop-tool/src/constants.py by chilli-axe and contributors.
// Modified for Rust from 2026-09-20; see NOTICE.md and Git history for changes.
//! Per-site configuration. Only MakePlayingCards is wired up for now; the struct
//! keeps room for the PrinterStudio family the reference tool also supports.

pub struct Site {
    pub base_url: &'static str,
    pub starting_route: &'static str,
    pub login_route: &'static str,
    pub logout_route: &'static str,
    pub cardstock_dropdown_id: &'static str,
    pub quantity_dropdown_id: &'static str,
    pub print_type_dropdown_id: &'static str,
    pub foil_option_value: &'static str,
    pub accept_settings_route: &'static str,
    pub supports_foil: bool,
}

impl Site {
    pub fn url(&self, route: &str) -> String {
        format!("{}/{route}", self.base_url)
    }

    pub fn starting_url(&self) -> String {
        self.url(self.starting_route)
    }

    pub fn login_url(&self) -> String {
        self.url(self.login_route)
    }

    pub fn logout_url(&self) -> String {
        self.url(self.logout_route)
    }
}

pub const MAKE_PLAYING_CARDS: Site = Site {
    base_url: "https://www.makeplayingcards.com",
    starting_route: "design/custom-blank-card.html",
    login_route: "login.aspx",
    logout_route: "logout.aspx",
    cardstock_dropdown_id: "dro_paper_type",
    quantity_dropdown_id: "dro_choosesize",
    print_type_dropdown_id: "dro_product_effect",
    foil_option_value: "EF_055",
    accept_settings_route: "products/pro_item_process_flow.aspx",
    supports_foil: true,
};
