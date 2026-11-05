//! `RebolLexer.word_callback` and `RedLexer.word_callback`: classify a word by
//! trying Python's `re.match` patterns in order.

use std::sync::LazyLock;

use onig::{Regex, SearchOptions};
use serde_json::Value;

use crate::engine::compile;
use crate::hooks::{Cx, Groups};
use crate::token::TokenType;

type Table = Vec<(Regex, TokenType)>;

fn build(rules: &[(&str, TokenType)]) -> Table {
    rules.iter().filter_map(|&(p, t)| compile(p, "").ok().map(|r| (r, t))).collect()
}

static REBOL: LazyLock<Table> = LazyLock::new(|| {
    build(&[
        (r".*:$", TokenType::GENERIC_SUBHEADING),
        (
            r"(native|alias|all|any|as-string|as-binary|bind|bound\?|case|catch|checksum|comment|debase|dehex|exclude|difference|disarm|either|else|enbase|foreach|remove-each|form|free|get|get-env|if|in|intersect|loop|minimum-of|maximum-of|mold|new-line|new-line\?|not|now|prin|print|reduce|compose|construct|repeat|reverse|save|script\?|set|shift|switch|throw|to-hex|trace|try|type\?|union|unique|unless|unprotect|unset|until|use|value\?|while|compress|decompress|secure|open|close|read|read-io|write-io|write|update|query|wait|input\?|exp|log-10|log-2|log-e|square-root|cosine|sine|tangent|arccosine|arcsine|arctangent|protect|lowercase|uppercase|entab|detab|connected\?|browse|launch|stats|get-modes|set-modes|to-local-file|to-rebol-file|encloak|decloak|create-link|do-browser|bind\?|hide|draw|show|size-text|textinfo|offset-to-caret|caret-to-offset|local-request-file|rgb-to-hsv|hsv-to-rgb|crypt-strength\?|dh-make-key|dh-generate-key|dh-compute-key|dsa-make-key|dsa-generate-key|dsa-make-signature|dsa-verify-signature|rsa-make-key|rsa-generate-key|rsa-encrypt)$",
            TokenType::NAME_BUILTIN,
        ),
        (
            r"(add|subtract|multiply|divide|remainder|power|and~|or~|xor~|minimum|maximum|negate|complement|absolute|random|head|tail|next|back|skip|at|pick|first|second|third|fourth|fifth|sixth|seventh|eighth|ninth|tenth|last|path|find|select|make|to|copy\*|insert|remove|change|poke|clear|trim|sort|min|max|abs|cp|copy)$",
            TokenType::NAME_FUNCTION,
        ),
        (
            r"(error|source|input|license|help|install|echo|Usage|with|func|throw-on-error|function|does|has|context|probe|\?\?|as-pair|mod|modulo|round|repend|about|set-net|append|join|rejoin|reform|remold|charset|array|replace|move|extract|forskip|forall|alter|first+|also|take|for|forever|dispatch|attempt|what-dir|change-dir|clean-path|list-dir|dirize|rename|split-path|delete|make-dir|delete-dir|in-dir|confirm|dump-obj|upgrade|what|build-tag|process-source|build-markup|decode-cgi|read-cgi|write-user|save-user|set-user-name|protect-system|parse-xml|cvs-date|cvs-version|do-boot|get-net-info|desktop|layout|scroll-para|get-face|alert|set-face|uninstall|unfocus|request-dir|center-face|do-events|net-error|decode-url|parse-header|parse-header-date|parse-email-addrs|import-email|send|build-attach-body|resend|show-popup|hide-popup|open-events|find-key-face|do-face|viewtop|confine|find-window|insert-event-func|remove-event-func|inform|dump-pane|dump-face|flag-face|deflag-face|clear-fields|read-net|vbug|path-thru|read-thru|load-thru|do-thru|launch-thru|load-image|request-download|do-face-alt|set-font|set-para|get-style|set-style|make-face|stylize|choose|hilight-text|hilight-all|unlight-text|focus|scroll-drag|clear-face|reset-face|scroll-face|resize-face|load-stock|load-stock-block|notify|request|flash|request-color|request-pass|request-text|request-list|request-date|request-file|dbug|editor|link-relative-path|emailer|parse-error)$",
            TokenType::KEYWORD_NAMESPACE,
        ),
        (r"(halt|quit|do|load|q|recycle|call|run|ask|parse|view|unview|return|exit|break)$", TokenType::NAME_EXCEPTION),
        (r"REBOL$", TokenType::GENERIC_HEADING),
        (r"to-.*", TokenType::KEYWORD),
        (r"(\+|-|\*|/|//|\*\*|and|or|xor|=\?|=|==|<>|<|>|<=|>=)$", TokenType::OPERATOR),
        (r".*\?$", TokenType::KEYWORD),
        (r".*\!$", TokenType::KEYWORD_TYPE),
        (r"'.*", TokenType::NAME_VARIABLE_INSTANCE),
        (r"#.*", TokenType::NAME_LABEL),
        (r"%.*", TokenType::NAME_DECORATOR),
    ])
});

static RED: LazyLock<Table> = LazyLock::new(|| {
    build(&[
        (r".*:$", TokenType::GENERIC_SUBHEADING),
        (
            r"(if|unless|either|any|all|while|until|loop|repeat|foreach|forall|func|function|does|has|switch|case|reduce|compose|get|set|print|prin|equal\?|not-equal\?|strict-equal\?|lesser\?|greater\?|lesser-or-equal\?|greater-or-equal\?|same\?|not|type\?|stats|bind|union|replace|charset|routine)$",
            TokenType::NAME_BUILTIN,
        ),
        (
            r"(make|random|reflect|to|form|mold|absolute|add|divide|multiply|negate|power|remainder|round|subtract|even\?|odd\?|and~|complement|or~|xor~|append|at|back|change|clear|copy|find|head|head\?|index\?|insert|length\?|next|pick|poke|remove|reverse|select|sort|skip|swap|tail|tail\?|take|trim|create|close|delete|modify|open|open\?|query|read|rename|update|write)$",
            TokenType::NAME_FUNCTION,
        ),
        (
            r"(yes|on|no|off|true|false|tab|cr|lf|newline|escape|slash|sp|space|null|none|crlf|dot|null-byte)$",
            TokenType::NAME_BUILTIN_PSEUDO,
        ),
        (
            r"(#system-global|#include|#enum|#define|#either|#if|#import|#export|#switch|#default|#get-definition)$",
            TokenType::KEYWORD_NAMESPACE,
        ),
        (
            r"(system|halt|quit|quit-return|do|load|q|recycle|call|run|ask|parse|raise-error|return|exit|break|alias|push|pop|probe|\?\?|spec-of|body-of|quote|forever)$",
            TokenType::NAME_EXCEPTION,
        ),
        (
            r"(action\?|block\?|char\?|datatype\?|file\?|function\?|get-path\?|zero\?|get-word\?|integer\?|issue\?|lit-path\?|lit-word\?|logic\?|native\?|op\?|paren\?|path\?|refinement\?|set-path\?|set-word\?|string\?|unset\?|any-struct\?|none\?|word\?|any-series\?)$",
            TokenType::KEYWORD,
        ),
        (r"(JNICALL|stdcall|cdecl|infix)$", TokenType::KEYWORD_NAMESPACE),
        (r"to-.*", TokenType::KEYWORD),
        (r"(\+|-\*\*|-|\*\*|//|/|\*|and|or|xor|=\?|===|==|=|<>|<=|>=|<<<|>>>|<<|>>|<|>%)$", TokenType::OPERATOR),
        (r".*\!$", TokenType::KEYWORD_TYPE),
        (r"'.*", TokenType::NAME_VARIABLE_INSTANCE),
        (r"#.*", TokenType::NAME_LABEL),
        (r"%.*", TokenType::NAME_DECORATOR),
        (r":.*", TokenType::GENERIC_SUBHEADING),
    ])
});

fn classify(table: &Table, cx: &mut Cx<'_>, g: &Groups<'_>) {
    let word = g.str(cx.text, 0).unwrap_or("");
    let ty = table
        .iter()
        .find(|(re, _)| re.match_with_options(word, 0, SearchOptions::SEARCH_OPTION_NONE, None).is_some())
        .map_or(TokenType::NAME_VARIABLE, |&(_, t)| t);
    cx.emit(ty, g.start(), g.end());
}

pub(super) fn rebol_word(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    classify(&REBOL, cx, g);
}

pub(super) fn red_word(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    classify(&RED, cx, g);
}
