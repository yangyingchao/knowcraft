// SPDX-License-Identifier: MIT

//! The syntax color palette resolved from the theme tokens. Source previews no
//! longer use GtkSourceView, but the syntax role colors remain part of the theme
//! for the color editor and Omarchy contrast mapping.

use gtk::gdk;

use super::{SourcePalette, ThemeTokens, blend};

pub(super) fn resolved_source_palette(
    tokens: &ThemeTokens,
    palette: Option<&SourcePalette>,
) -> SourcePalette {
    let fallback = SourcePalette {
        statement: tokens.accent.clone(),
        string: blend(&tokens.accent, &tokens.text, 0.48),
        constant: blend(&tokens.accent, &tokens.text, 0.18),
        type_color: blend(&tokens.accent, &tokens.text, 0.24),
        preprocessor: blend(&tokens.accent, &tokens.text, 0.32),
    };
    let palette = palette.unwrap_or(&fallback);
    let resolve = |token: &Option<String>, fallback: &str| {
        token
            .as_deref()
            .filter(|value| gdk::RGBA::parse(*value).is_ok())
            .unwrap_or(fallback)
            .to_owned()
    };
    SourcePalette {
        statement: resolve(&tokens.syntax_keyword, &palette.statement),
        string: resolve(&tokens.syntax_string, &palette.string),
        constant: resolve(&tokens.syntax_constant, &palette.constant),
        type_color: resolve(&tokens.syntax_type, &palette.type_color),
        preprocessor: resolve(&tokens.syntax_preprocessor, &palette.preprocessor),
    }
}
