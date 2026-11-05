// SPDX-License-Identifier: MIT

use crate::ui::theme::{ThemeTokens, color_to_hex, resolved_source_palette, validate_tokens};

#[test]
fn legacy_custom_themes_keep_independent_fallbacks_and_preserve_overrides() {
    let source = r##"
name = "Legacy"
background = "#000000"
surface = "#111111"
text = "#ffffff"
accent = "#336699"
muted = "#222222"
highlight = "#333333"
border = "#444444"
dim_text = "#888888"
"##;
    let mut tokens: ThemeTokens = toml::from_str(source).expect("legacy theme");
    let derived = resolved_source_palette(&tokens, None);
    tokens.syntax_string = Some("rgb(0,255,0)".into());
    let resolved = resolved_source_palette(&tokens, None);
    assert_eq!(resolved.statement, derived.statement);
    assert_eq!(resolved.type_color, derived.type_color);
    assert_eq!(resolved.constant, derived.constant);
    assert_eq!(resolved.preprocessor, derived.preprocessor);
    assert_eq!(color_to_hex(&resolved.string), "#00ff00");
    tokens.initialize_syntax_colors();
    let restored: ThemeTokens =
        toml::from_str(&toml::to_string(&tokens).expect("serialize theme")).expect("restore theme");
    assert_eq!(restored, tokens);
    assert!(validate_tokens(&restored).is_ok());
    for slot in ["keyword", "string", "constant", "type", "preprocessor"] {
        let invalid: ThemeTokens =
            toml::from_str(&format!("{source}\nsyntax_{slot} = \"not-a-color\"\n"))
                .expect("parse invalid-color fixture");
        assert!(validate_tokens(&invalid).is_err(), "{slot}");
    }
}
