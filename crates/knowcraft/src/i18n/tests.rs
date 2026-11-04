// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn detects_supported_locales_and_honors_posix_precedence() {
    for (language, all, messages, lang, expected) in [
        ("", "", "", "en_US.UTF-8", "en"),
        ("", "", "", "ZH_cn.UTF-8", "zh-CN"),
        // 界面只保留英文与简体中文；繁体环境一律回落到简体中文。
        ("", "", "", "zh_CN.GB18030", "zh-CN"),
        ("", "", "", "zh_TW.UTF-8", "zh-CN"),
        ("", "zh_HK.UTF-8", "", "en_US", "zh-CN"),
        ("", "", "", "zh_MO", "zh-CN"),
        ("", "", "", "zh_MY.UTF-8", "zh-CN"),
        ("zh-Hans", "", "", "en_US.UTF-8", "zh-CN"),
        ("xx:zh-Hant:ja", "", "", "de_DE", "zh-CN"),
        // LC_ALL 优先于 LC_MESSAGES，LC_MESSAGES 优先于 LANG。
        ("", "zh_CN.UTF-8", "en_US", "en_US", "zh-CN"),
        ("", "", "zh_CN.UTF-8", "en_US", "zh-CN"),
        // GNU LANGUAGE 优先列表先于标准变量。
        ("xx:zh-CN:zz", "", "", "en_US", "zh-CN"),
        // 显式 POSIX locale 抑制 GNU LANGUAGE。
        ("zh-CN", "C", "", "en_US", "en"),
        ("zh-CN", "POSIX", "", "en_US", "en"),
        // 优先列表与标准变量都不支持时回落英文。
        ("xx:zz", "", "", "vi_VN", "en"),
        ("", "", "", "unsupported", "en"),
        ("", "", "", "", "en"),
    ] {
        assert_eq!(
            resolve_locale(language, all, messages, lang),
            expected,
            "{language}/{all}/{messages}/{lang}"
        );
    }
}

#[test]
fn manual_language_does_not_depend_on_system_locale() {
    for (choice, locale) in [
        (Language::English, "en"),
        (Language::SimplifiedChinese, "zh-CN"),
    ] {
        assert_eq!(choice.locale(), locale);
        assert_eq!(
            serde_json::from_str::<Language>(
                &serde_json::to_string(&choice).expect("serialize language")
            )
            .expect("deserialize language"),
            choice
        );
    }
    assert_eq!(
        serde_json::from_str::<Language>("\"future-language\"").expect("recover unknown language"),
        Language::Auto
    );
}

#[test]
fn plural_categories_follow_english_and_chinese_forms() {
    for (locale, values) in [
        ("en", vec![(0, "other"), (1, "one"), (2, "other")]),
        ("zh-CN", vec![(1, "other")]),
    ] {
        for (count, expected) in values {
            assert_eq!(plural_category(locale, count), expected, "{locale}/{count}");
        }
    }
    assert_eq!(
        rust_i18n::t!("counts.items.other", locale = "en", count = 2),
        "2 items"
    );
    assert_eq!(
        rust_i18n::t!("counts.items.few", locale = "zh-CN", count = 22),
        "22 个项目"
    );
}

#[test]
fn lists_use_the_language_separator() {
    let items = || ["2 folders", "3 files", "1 link"].map(str::to_owned);
    assert_eq!(list_in("en", items()), "2 folders, 3 files, 1 link");
    assert_eq!(list_in("zh-CN", items()), "2 folders、3 files、1 link");
    assert_eq!(list_in("en", []), "");
    assert_eq!(capitalize_first("hello world".to_owned()), "Hello world");
}

#[test]
fn sentences_join_without_a_space_after_a_full_width_stop() {
    for (first, second, expected) in [
        ("复制失败。", "请重试。", "复制失败。请重试。"),
        ("确定吗？", "此操作无法撤销。", "确定吗？此操作无法撤销。"),
        ("权限不足", "请重试。", "权限不足。请重试。"),
    ] {
        assert_eq!(join_sentences_in("zh-CN", first, second), expected);
    }
}

#[test]
fn interpolation_preserves_user_text_without_retranslating_or_expanding_it() {
    let filename = "Language %{value2} <&> 日本語.txt";
    assert_eq!(
        rust_i18n::t!(
            "This runs “%{value1}” on %{value2}.",
            locale = "en",
            value1 = filename,
            value2 = "1 file"
        ),
        "This runs “Language %{value2} <&> 日本語.txt” on 1 file.",
    );
}

#[test]
fn english_fallback_keeps_missing_messages_readable() {
    assert_eq!(
        rust_i18n::t!("Language", locale = "unsupported"),
        "Language"
    );
    assert_eq!(
        rust_i18n::t!("A message not yet translated", locale = "unsupported"),
        "A message not yet translated"
    );
}

#[test]
fn file_sizes_use_the_language_decimal_separator_and_unit_symbols() {
    for (locale, bytes, expected) in [
        ("en", 999, "999 B"),
        ("en", 1_500, "1.5 kB"),
        ("en", 2_000, "2 kB"),
        ("en", 999_960, "1 MB"),
    ] {
        assert_eq!(file_size_in(locale, bytes), expected, "{locale}/{bytes}");
    }
    assert_eq!(transfer_rate_in("en", 1_000), "1 kB/s");
}

#[test]
fn counts_and_numbers_group_digits_by_language() {
    assert_eq!(count_in("en", "files", 3_000), "3,000 files");
    assert_eq!(count_in("en", "files", 999), "999 files");
    assert_eq!(count_in("zh-CN", "files", 3_000), "3,000 个文件");
    assert_eq!(decimal_in("en", 400.0, 1), "400.0");
    assert_eq!(decimal_in("zh-CN", 1_234_567.3, 1), "1,234,567.3");
    assert_eq!(percent_in("en", 42), "42%");
}

#[test]
fn durations_use_the_language_units() {
    for (locale, seconds, expected) in [
        ("en", 0, "0s"),
        ("en", 125, "2m 5s"),
        ("en", 3_725, "1h 2m"),
        ("zh-CN", 45, "45秒"),
        ("zh-CN", 125, "2分5秒"),
        ("zh-CN", 3_725, "1小时2分"),
    ] {
        assert_eq!(duration_in(locale, seconds), expected, "{locale}/{seconds}");
    }
}
