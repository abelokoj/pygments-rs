//! Expected tokens obtained from Pygments 2.19.2 (`get_tokens`).

fn lex<'t>(alias: &str, text: &'t str) -> Vec<(&'static str, &'t str)> {
    let lexer = crate::lexer_by_name(alias).expect("lexer exists");
    assert_eq!(lexer.preprocess(text), text, "test inputs are already normalized");
    lexer.tokenize(text).map(|(ty, v)| (ty.short_path(), v)).collect()
}

fn check(alias: &str, text: &str, expected: &[(&str, &str)]) {
    assert_eq!(lex(alias, text), expected);
}

#[test]
fn yaml_block() {
    check(
        "yaml",
        "a:\n  b: 1\n  c: |\n    x\n    y\nd: [1, 2]\n---\n- e\n",
        &[
            ("Name.Tag", "a"),
            ("Punctuation", ":"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", "  "),
            ("Name.Tag", "b"),
            ("Punctuation", ":"),
            ("Text.Whitespace", " "),
            ("Literal.Scalar.Plain", "1"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", "  "),
            ("Name.Tag", "c"),
            ("Punctuation", ":"),
            ("Text.Whitespace", " "),
            ("Punctuation.Indicator", "|"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", "    "),
            ("Name.Constant", "x"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", "    "),
            ("Name.Constant", "y"),
            ("Text.Whitespace", "\n"),
            ("Name.Tag", "d"),
            ("Punctuation", ":"),
            ("Text.Whitespace", " "),
            ("Punctuation.Indicator", "["),
            ("Name.Variable", "1"),
            ("Punctuation.Indicator", ","),
            ("Text.Whitespace", " "),
            ("Name.Variable", "2"),
            ("Punctuation.Indicator", "]"),
            ("Text.Whitespace", "\n"),
            ("Name.Namespace", "---"),
            ("Text.Whitespace", "\n"),
            ("Punctuation.Indicator", "-"),
            ("Text.Whitespace", " "),
            ("Literal.Scalar.Plain", "e"),
            ("Text.Whitespace", "\n"),
        ],
    );
}

#[test]
fn yaml_folded_dedent() {
    check(
        "yaml",
        "k: >2\n   t\n\nz: plain\n  more\n",
        &[
            ("Name.Tag", "k"),
            ("Punctuation", ":"),
            ("Text.Whitespace", " "),
            ("Punctuation.Indicator", ">"),
            ("Punctuation.Indicator", "2"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", "   "),
            ("Name.Constant", "t"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", "\n"),
            ("Name.Tag", "z"),
            ("Punctuation", ":"),
            ("Text.Whitespace", " "),
            ("Literal.Scalar.Plain", "plain"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", "  "),
            ("Literal.Scalar.Plain", "more"),
            ("Text.Whitespace", "\n"),
        ],
    );
}

#[test]
fn sass() {
    check(
        "sass",
        "a\n  b: c\n// x\n  y\nd\n",
        &[
            ("Text.Whitespace", ""),
            ("Name.Tag", "a"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", "  "),
            ("Name.Attribute", ""),
            ("Name.Attribute", "b"),
            ("Operator", ":"),
            ("Text.Whitespace", " "),
            ("Name", "c"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", ""),
            ("Comment.Single", "// x"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", "  "),
            ("Comment.Single", "y"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", ""),
            ("Name.Tag", "d"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", ""),
        ],
    );
}

#[test]
fn haml() {
    check(
        "haml",
        "%p\n  -# c\n    d\n%q\n",
        &[
            ("Text.Whitespace", ""),
            ("Name.Tag", "%p"),
            ("Text", "\n"),
            ("Text.Whitespace", "  "),
            ("Comment.Preproc", "-# c\n"),
            ("Text.Whitespace", "    "),
            ("Comment.Preproc", "d"),
            ("Text", "\n"),
            ("Text.Whitespace", ""),
            ("Name.Tag", "%q"),
            ("Text", "\n"),
            ("Text.Whitespace", ""),
        ],
    );
}

#[test]
fn slim() {
    check(
        "slim",
        "p\n  / note\n    more\nb\n",
        &[
            ("Text.Whitespace", ""),
            ("Name.Tag", "p"),
            ("Text", "\n"),
            ("Text.Whitespace", "  "),
            ("Comment.Preproc", "/ note\n"),
            ("Text.Whitespace", "    "),
            ("Comment.Preproc", "more"),
            ("Text", "\n"),
            ("Text.Whitespace", ""),
            ("Name.Tag", "b"),
            ("Text", "\n"),
            ("Text.Whitespace", ""),
        ],
    );
}

#[test]
fn pug() {
    check(
        "pug",
        "p\n  // c\n    d\nq\n",
        &[
            ("Text.Whitespace", ""),
            ("Name.Tag", "p"),
            ("Text", "\n"),
            ("Text.Whitespace", "  "),
            ("Comment", "// c\n"),
            ("Text.Whitespace", "    "),
            ("Comment", "d"),
            ("Text", "\n"),
            ("Text.Whitespace", ""),
            ("Name.Tag", "q"),
            ("Text", "\n"),
            ("Text.Whitespace", ""),
        ],
    );
}

#[test]
fn ruby_heredoc() {
    check(
        "ruby",
        "f(<<-A, <<~B)\n  one\n  A\n  two\n  B\nx\n",
        &[
            ("Name", "f"),
            ("Punctuation", "("),
            ("Operator", "<<-"),
            ("Literal.String.Heredoc", ""),
            ("Literal.String.Delimiter", "A"),
            ("Literal.String.Heredoc", ""),
            ("Punctuation", ","),
            ("Text.Whitespace", " "),
            ("Operator", "<<~"),
            ("Literal.String.Heredoc", ""),
            ("Literal.String.Delimiter", "B"),
            ("Literal.String.Heredoc", ""),
            ("Punctuation", ")"),
            ("Text.Whitespace", "\n"),
            ("Literal.String.Heredoc", "  one\n"),
            ("Literal.String.Delimiter", "  A\n"),
            ("Literal.String.Heredoc", "  two\n"),
            ("Literal.String.Delimiter", "  B\n"),
            ("Name", "x"),
            ("Text.Whitespace", "\n"),
        ],
    );
}

#[test]
fn ruby_heredoc_unterminated() {
    check(
        "ruby",
        "x = <<EOS\nhi\n",
        &[
            ("Name", "x"),
            ("Text.Whitespace", " "),
            ("Operator", "="),
            ("Text.Whitespace", " "),
            ("Operator", "<<"),
            ("Literal.String.Heredoc", ""),
            ("Literal.String.Delimiter", "EOS"),
            ("Literal.String.Heredoc", ""),
            ("Text.Whitespace", "\n"),
            ("Error", "hi\n"),
            ("Name", "hi"),
            ("Text.Whitespace", "\n"),
        ],
    );
}

#[test]
fn ruby_intp() {
    check(
        "ruby",
        "%r{a#{b}c}i\n%Q(x#{y})\n%(z)\n",
        &[
            ("Literal.String.Regex", "%r{"),
            ("Literal.String.Regex", "a"),
            ("Literal.String.Interpol", "#{"),
            ("Name", "b"),
            ("Literal.String.Interpol", "}"),
            ("Literal.String.Regex", "c"),
            ("Literal.String.Regex", "}i"),
            ("Text.Whitespace", "\n"),
            ("Literal.String.Other", "%Q("),
            ("Literal.String.Other", "x"),
            ("Literal.String.Interpol", "#{"),
            ("Name", "y"),
            ("Literal.String.Interpol", "}"),
            ("Literal.String.Other", ")"),
            ("Text.Whitespace", "\n"),
            ("Literal.String.Other", "%("),
            ("Literal.String.Other", "z"),
            ("Literal.String.Other", ")"),
            ("Text.Whitespace", "\n"),
        ],
    );
}

#[test]
fn crystal() {
    check(
        "crystal",
        "x = <<-EOS\n  hi\n  EOS\n",
        &[
            ("Name", "x"),
            ("Text.Whitespace", " "),
            ("Operator", "="),
            ("Text.Whitespace", " "),
            ("Operator", "<<-"),
            ("Literal.String.Heredoc", ""),
            ("Literal.String.Delimiter", "EOS"),
            ("Literal.String.Heredoc", ""),
            ("Text.Whitespace", "\n"),
            ("Literal.String.Heredoc", "  hi\n"),
            ("Literal.String.Delimiter", "  EOS\n"),
        ],
    );
}

#[test]
fn terraform() {
    check(
        "terraform",
        "a = <<EOF\nhi\n  EOF\n",
        &[
            ("Name.Attribute", "a"),
            ("Text.Whitespace", " "),
            ("Operator", "="),
            ("Text.Whitespace", " "),
            ("Operator", "<<"),
            ("Literal.String.Delimiter", "EOF"),
            ("Literal.String.Heredoc", "\n"),
            ("Literal.String.Heredoc", "hi\n"),
            ("Literal.String.Delimiter", "  EOF\n"),
        ],
    );
}

#[test]
fn perl6() {
    check(
        "perl6",
        "my $s = q{a{b}c};\n#`( c )\nmy $t = q:to/END/;\n  x\n  END\nsay 1;\n",
        &[
            ("Keyword", "my"),
            ("Text", " "),
            ("Name.Variable", "$s"),
            ("Text", " "),
            ("Text", "="),
            ("Text", " "),
            ("Literal.String", "q{a{b}c}"),
            ("Text", ";"),
            ("Text", "\n"),
            ("Comment.Multiline", "#`( c )"),
            ("Text", "\n"),
            ("Keyword", "my"),
            ("Text", " "),
            ("Name.Variable", "$t"),
            ("Text", " "),
            ("Text", "="),
            ("Text", " "),
            ("Literal.String", "q:to/END/;\n  x\n  END\n"),
            ("Name.Builtin", "say"),
            ("Text", " "),
            ("Literal.Number.Integer", "1"),
            ("Text", ";"),
            ("Text", "\n"),
        ],
    );
}

#[test]
fn perl6_token() {
    check(
        "perl6",
        "token t { a { $x } b }\n",
        &[
            ("Keyword", "token"),
            ("Name", " t"),
            ("Text", " "),
            ("Text", "{"),
            ("Literal.String.Regex", " "),
            ("Literal.String.Regex", "a"),
            ("Literal.String.Regex", " "),
            ("Text", "{"),
            ("Text", " "),
            ("Name.Variable", "$x"),
            ("Text", " "),
            ("Text", "}"),
            ("Literal.String.Regex", " "),
            ("Literal.String.Regex", "b"),
            ("Literal.String.Regex", " "),
            ("Text", "}"),
            ("Text", "\n"),
        ],
    );
}

#[test]
fn snowball() {
    check(
        "snowball",
        "stringescapes {}\ndefine x as ( 'a{q}b' )\n",
        &[
            ("Keyword.Reserved", "stringescapes"),
            ("Text.Whitespace", " "),
            ("Literal.String.Escape", "{"),
            ("Literal.String.Escape", "}"),
            ("Text.Whitespace", "\n"),
            ("Keyword.Reserved", "define"),
            ("Text.Whitespace", " "),
            ("Name", "x"),
            ("Text.Whitespace", " "),
            ("Keyword.Reserved", "as"),
            ("Text.Whitespace", " "),
            ("Punctuation", "("),
            ("Text.Whitespace", " "),
            ("Literal.String.Single", "'"),
            ("Literal.String.Single", "a"),
            ("Literal.String.Escape", "{"),
            ("Literal.String.Escape", "q}"),
            ("Literal.String.Single", "b"),
            ("Literal.String.Single", "'"),
            ("Text.Whitespace", " "),
            ("Punctuation", ")"),
            ("Text.Whitespace", "\n"),
        ],
    );
}

#[test]
fn markdown() {
    check(
        "markdown",
        "```python\nx = 1\n```\n\n```nolang extra\nx\n```\n",
        &[
            ("Literal.String.Backtick", "```"),
            ("Literal.String.Backtick", "python"),
            ("Text.Whitespace", "\n"),
            ("Name", "x"),
            ("Text", " "),
            ("Operator", "="),
            ("Text", " "),
            ("Literal.Number.Integer", "1"),
            ("Text.Whitespace", "\n"),
            ("Literal.String.Backtick", "```\n"),
            ("Literal.String.Backtick", "\n```"),
            ("Literal.String.Backtick", "nolang"),
            ("Text.Whitespace", " "),
            ("Text", "extra"),
            ("Text.Whitespace", "\n"),
            ("Literal.String", "x\n"),
            ("Literal.String.Backtick", "```\n"),
        ],
    );
}

#[test]
fn rst() {
    check(
        "rst",
        ".. code-block:: python\n\n    x = 1\n    y\n\nafter\n",
        &[
            ("Punctuation", ".."),
            ("Text", " "),
            ("Operator.Word", "code-block"),
            ("Punctuation", "::"),
            ("Text", " "),
            ("Keyword", "python"),
            ("Text", "\n\n"),
            ("Text", "    "),
            ("Name", "x"),
            ("Text", " "),
            ("Operator", "="),
            ("Text", " "),
            ("Literal.Number.Integer", "1"),
            ("Text.Whitespace", "\n"),
            ("Text", "    "),
            ("Name", "y"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", "\n"),
            ("Text", "after"),
            ("Text.Whitespace", "\n"),
        ],
    );
}

#[test]
fn tiddler() {
    check(
        "tid",
        "```python\nx\n```\n<style>\na {}\n</style>\n",
        &[
            ("Literal.String", "```"),
            ("Literal.String", "python"),
            ("Text", "\n"),
            ("Name", "x"),
            ("Text.Whitespace", "\n"),
            ("Literal.String", "```"),
            ("Text.Whitespace", "\n"),
            ("Literal.String", "<style>"),
            ("Literal.String", "\n"),
            ("Name.Tag", "a"),
            ("Text.Whitespace", " "),
            ("Punctuation", "{"),
            ("Punctuation", "}"),
            ("Text.Whitespace", "\n"),
            ("Literal.String", "</style>"),
            ("Text.Whitespace", "\n"),
        ],
    );
}

#[test]
fn tiddler_nolang() {
    check(
        "tid",
        "```nolang\nx\n```\n",
        &[
            ("Literal.String", "```"),
            ("Literal.String", "nolang"),
            ("Text", "\n"),
            ("Literal.String", "x\n"),
            ("Text.Whitespace", "\n"),
        ],
    );
}

#[test]
fn wikitext() {
    check(
        "wikitext",
        "<syntaxhighlight lang=\"python\">x = 1</syntaxhighlight>\n<score lang=\"ABC\">X</score>\n<source>y</source>\n",
        &[
            ("Punctuation", "<"),
            ("Name.Tag", "syntaxhighlight"),
            ("Text.Whitespace", " "),
            ("Name.Attribute", "lang"),
            ("Operator", "="),
            ("Literal.String.Double", "\""),
            ("Literal.String.Double", "python"),
            ("Literal.String.Double", "\""),
            ("Punctuation", ">"),
            ("Name", "x"),
            ("Text", " "),
            ("Operator", "="),
            ("Text", " "),
            ("Literal.Number.Integer", "1"),
            ("Punctuation", "</"),
            ("Name.Tag", "syntaxhighlight"),
            ("Punctuation", ">"),
            ("Text", "\n"),
            ("Punctuation", "<"),
            ("Name.Tag", "score"),
            ("Text.Whitespace", " "),
            ("Name.Attribute", "lang"),
            ("Operator", "="),
            ("Literal.String.Double", "\""),
            ("Literal.String.Double", "ABC"),
            ("Literal.String.Double", "\""),
            ("Punctuation", ">"),
            ("Text", "X"),
            ("Punctuation", "</"),
            ("Name.Tag", "score"),
            ("Punctuation", ">"),
            ("Text", "\n"),
            ("Punctuation", "<"),
            ("Name.Tag", "source"),
            ("Punctuation", ">"),
            ("Text", "y"),
            ("Punctuation", "</"),
            ("Name.Tag", "source"),
            ("Punctuation", ">"),
            ("Text", "\n"),
        ],
    );
}

#[test]
fn http() {
    check(
        "http",
        "GET / HTTP/1.1\nContent-Type: application/foo+xml; charset=utf-8\n  cont\n\n<a b=\"c\"/>\n",
        &[
            ("Name.Function", "GET"),
            ("Text", " "),
            ("Name.Namespace", "/"),
            ("Text", " "),
            ("Keyword.Reserved", "HTTP"),
            ("Operator", "/"),
            ("Literal.Number", "1.1"),
            ("Text", "\n"),
            ("Name.Attribute", "Content-Type"),
            ("Text", ""),
            ("Operator", ":"),
            ("Text", " "),
            ("Literal", "application/foo+xml; charset=utf-8"),
            ("Text", "\n"),
            ("Text", "  "),
            ("Literal", "cont"),
            ("Text", "\n"),
            ("Text", "\n"),
            ("Name.Tag", "<a"),
            ("Text.Whitespace", " "),
            ("Name.Attribute", "b="),
            ("Literal.String", "\"c\""),
            ("Name.Tag", "/>"),
            ("Text.Whitespace", "\n"),
        ],
    );
}

#[test]
fn notmuch() {
    let text = "\u{c}message{ id:x depth:0 match:1 excluded:0 filename:/a\n\u{c}header{\nSubject: hi\n\u{c}header}\n\u{c}body{\n\u{c}part{ ID: 1, Content-type: text/plain\nhello\n\u{c}part}\n\u{c}body}\n\u{c}message}\n";
    // The body language is guessed with every lexer's `analyse_text`: check the rest.
    let got: Vec<_> = lex("notmuch", text).into_iter().filter(|t| t.1 != "hello\n").collect();
    let expected = [
        ("Keyword", "\u{c}message{ "),
        ("Name.Attribute", "id:"),
        ("Literal.String", "x"),
        ("Name.Attribute", " depth:"),
        ("Literal.Number.Integer", "0"),
        ("Name.Attribute", " match:"),
        ("Literal.Number.Integer", "1"),
        ("Name.Attribute", " excluded:"),
        ("Literal.Number.Integer", "0"),
        ("Name.Attribute", " filename:"),
        ("Literal.String", "/a\n"),
        ("Keyword", "\u{c}header{\n"),
        ("Name.Attribute", "Subject: "),
        ("Literal.String", "hi\n"),
        ("Keyword", "\u{c}header}\n"),
        ("Keyword", "\u{c}body{\n"),
        ("Keyword", "\u{c}part{ "),
        ("Name.Attribute", "ID: "),
        ("Literal.Number.Integer", "1"),
        ("Punctuation", ", "),
        ("Name.Attribute", "Content-type: "),
        ("Literal.String", "text/plain\n"),
        ("Text", "hello\n"),
        ("Keyword", "\u{c}part}\n"),
        ("Keyword", "\u{c}body}\n"),
        ("Keyword", "\u{c}message}\n"),
    ];
    let expected: Vec<_> = expected.into_iter().filter(|t| t.1 != "hello\n").collect();
    assert_eq!(got, expected);
}

#[test]
fn mime() {
    check(
        "mime",
        "Content-Type: multipart/mixed; boundary=\"XX\"\nContent-Transfer-Encoding: 7bit\nX-Other: y\n\npre\n--XX\nContent-Type: text/plain\n\nhello\n--XX--\nend\n",
        &[
            ("Name.Tag", "Content-Type:"),
            ("Text.Whitespace", " "),
            ("Text.Whitespace", ""),
            ("Name.Label", "multipart"),
            ("Literal.String.Delimiter", "/"),
            ("Name.Label", "mixed"),
            ("Text", ";"),
            ("Text.Whitespace", " "),
            ("Name.Attribute", "boundary"),
            ("Operator", "="),
            ("Literal.String", "\"XX\""),
            ("Text.Whitespace", "\n"),
            ("Name.Tag", "Content-Transfer-Encoding:"),
            ("Text.Whitespace", " "),
            ("Name.Constant", "7bit"),
            ("Text.Whitespace", "\n"),
            ("Comment", "X-Other: y\n"),
            ("Text.Whitespace", "\n"),
            ("Text", "pre\n"),
            ("Literal.String.Delimiter", "--XX\n"),
            ("Name.Tag", "Content-Type:"),
            ("Text.Whitespace", " "),
            ("Text.Whitespace", ""),
            ("Name.Label", "text"),
            ("Literal.String.Delimiter", "/"),
            ("Name.Label", "plain"),
            ("Text.Whitespace", "\n"),
            ("Text.Whitespace", "\n"),
            ("Text", "hello\n"),
            ("Literal.String.Delimiter", "--XX--\n"),
            ("Text", "end\n"),
        ],
    );
}

#[test]
fn postgres() {
    check(
        "postgresql",
        "CREATE FUNCTION f() RETURNS int AS $$ SELECT 1 $$ LANGUAGE sql;\nDO $x$ BEGIN END $x$;\nSELECT $q$ s $q$;\n",
        &[
            ("Keyword", "CREATE"),
            ("Text.Whitespace", " "),
            ("Keyword", "FUNCTION"),
            ("Text.Whitespace", " "),
            ("Name", "f"),
            ("Punctuation", "("),
            ("Punctuation", ")"),
            ("Text.Whitespace", " "),
            ("Keyword", "RETURNS"),
            ("Text.Whitespace", " "),
            ("Name.Builtin", "int"),
            ("Text.Whitespace", " "),
            ("Keyword", "AS"),
            ("Text.Whitespace", " "),
            ("Literal.String", "$"),
            ("Literal.String.Delimiter", ""),
            ("Literal.String", "$"),
            ("Text.Whitespace", " "),
            ("Keyword", "SELECT"),
            ("Text.Whitespace", " "),
            ("Literal.Number.Float", "1"),
            ("Text.Whitespace", " "),
            ("Literal.String", "$"),
            ("Literal.String.Delimiter", ""),
            ("Literal.String", "$"),
            ("Text.Whitespace", " "),
            ("Keyword", "LANGUAGE"),
            ("Text.Whitespace", " "),
            ("Keyword", "sql"),
            ("Punctuation", ";"),
            ("Text.Whitespace", "\n"),
            ("Keyword", "DO"),
            ("Text.Whitespace", " "),
            ("Literal.String", "$"),
            ("Literal.String.Delimiter", "x"),
            ("Literal.String", "$"),
            ("Text.Whitespace", " "),
            ("Keyword", "BEGIN"),
            ("Text.Whitespace", " "),
            ("Keyword", "END"),
            ("Text.Whitespace", " "),
            ("Literal.String", "$"),
            ("Literal.String.Delimiter", "x"),
            ("Literal.String", "$"),
            ("Punctuation", ";"),
            ("Text.Whitespace", "\n"),
            ("Keyword", "SELECT"),
            ("Text.Whitespace", " "),
            ("Literal.String", "$"),
            ("Literal.String.Delimiter", "q"),
            ("Literal.String", "$"),
            ("Text.Whitespace", " "),
            ("Name", "s"),
            ("Text.Whitespace", " "),
            ("Literal.String", "$"),
            ("Literal.String.Delimiter", "q"),
            ("Literal.String", "$"),
            ("Punctuation", ";"),
            ("Text.Whitespace", "\n"),
        ],
    );
}
