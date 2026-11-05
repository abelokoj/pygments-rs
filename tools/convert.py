"""Convert Pygments lexer tables, token types and styles into data for pygments-rs.

Dev-only tool. The library never runs Python; this script's output under data/
and src/generated/ is checked in.

Usage (from the repo root):
    python -I tools/convert.py --pygments PATH_TO_PYGMENTS_CHECKOUT

PATH_TO_PYGMENTS_CHECKOUT must be a Pygments source tree at the tag recorded in
PYGMENTS_VERSION (the one whose tests/ the golden tests use).
"""

import argparse
import ast
import fnmatch
import importlib
import inspect
import json
import re
import sys
import textwrap
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Python re flag bits we forward to Oniguruma; anything else is reported.
RE_FLAGS = {re.IGNORECASE: 'i', re.MULTILINE: 'm', re.DOTALL: 's', re.VERBOSE: 'x'}


def load_pygments(path):
    sys.path.insert(0, str(Path(path).resolve()))
    import pygments
    import pygments.lexer as plexer
    import pygments.lexers as plexers
    import pygments.styles as pstyles
    import pygments.token as ptoken
    return pygments, plexer, plexers, pstyles, ptoken


def tok_name(tok):
    return str(tok)  # e.g. "Token.Keyword.Constant"


def flag_str(flags):
    out = ''
    for bit, ch in RE_FLAGS.items():
        if flags & bit:
            out += ch
    return out


class Converter:
    def __init__(self, plexer, ptoken):
        self.plexer = plexer
        self.ptoken = ptoken

    # -- actions ---------------------------------------------------------
    def action(self, act, ctx):
        """Serialize a rule action. ctx collects needs (callbacks) for the report."""
        if act is None:
            return None
        if type(act) is self.ptoken._TokenType:
            return tok_name(act)
        if not callable(act):
            raise ValueError(f'unknown action {act!r}')
        qual = getattr(act, '__qualname__', '')
        cells = closure_vars(act)
        if qual == 'bygroups.<locals>.callback':
            return {'bygroups': [self.action(a, ctx) for a in cells['args']]}
        if qual == 'using.<locals>.callback':
            out = {}
            other = cells.get('_other')
            out['using'] = 'this' if other is None else other.__name__
            stack = cells['gt_kwargs'].get('stack')
            if stack is not None:
                out['stack'] = list(stack)
            if cells.get('kwargs'):
                out['options'] = {k: jsonable(v) for k, v in cells['kwargs'].items()}
            return out
        # A Python callback: needs a Rust port, keyed by module + qualname.
        key = f'{act.__module__.rsplit(".", 1)[-1]}::{qual}'
        out = {'callback': key}
        args = {k: jsonable(v) for k, v in cells.items() if not callable(v) or type(v) is self.ptoken._TokenType}
        if args:
            out['args'] = args
        ctx['callbacks'].add(key)
        ctx['callback_fns'].append(act)
        return out

    # -- states ----------------------------------------------------------
    def new_state(self, ns, unprocessed, processed, regexes, ctx):
        p = self.plexer
        if isinstance(ns, p.combined):
            name = f'_tmp_{ctx["tmp"]}'
            ctx['tmp'] += 1
            rules = []
            for st in ns:
                rules.extend(self.state(unprocessed, processed, st, regexes, ctx))
            processed[name] = rules
            return {'push': [name]}
        if isinstance(ns, str):
            if ns == '#pop':
                return {'pop': 1}
            if ns.startswith('#pop:'):
                return {'pop': int(ns[5:])}
            return {'push': [ns]}  # a state name or '#push'
        if isinstance(ns, tuple):
            return {'push': list(ns)}
        raise ValueError(f'unknown new state {ns!r}')

    def regex(self, pattern, flags, regexes):
        p = self.plexer
        if isinstance(pattern, p.Future):
            pattern = pattern.get()
        re.compile(pattern, flags)  # same validation Pygments does
        pattern = strip_surrogates(pattern)
        key = (pattern, flag_str(flags))
        if key not in regexes:
            regexes[key] = len(regexes)
        return regexes[key]

    def state(self, unprocessed, processed, name, regexes, ctx):
        p = self.plexer
        if name in processed:
            return processed[name]
        rules = processed[name] = []
        for tdef in unprocessed[name]:
            if isinstance(tdef, p.include):
                rules.extend(self.state(unprocessed, processed, str(tdef), regexes, ctx))
                continue
            if isinstance(tdef, p._inherit):
                continue
            if isinstance(tdef, p.default):
                rule = {'re': self.regex('', ctx['flags'], regexes)}
                rule.update(self.new_state(tdef.state, unprocessed, processed, regexes, ctx))
                rules.append(rule)
                continue
            rule = {'re': self.regex(tdef[0], ctx['flags'], regexes)}
            act = self.action(tdef[1], ctx)
            if act is not None:
                rule['action'] = act
            if len(tdef) > 2:
                rule.update(self.new_state(tdef[2], unprocessed, processed, regexes, ctx))
            rules.append(rule)
        return rules

    def regex_lexer(self, cls, inst, ctx):
        if getattr(cls, 'token_variants', None):
            # Variant lexers (C#, Nemerle, Inform 7) pick a token set per option;
            # we convert the one a default-constructed lexer uses.
            key = next(k for k, v in cls._all_tokens.items() if v is inst._tokens)
            unprocessed = cls.tokens[key]
            ctx['variant'] = key
        else:
            unprocessed = cls.get_tokendefs()
        processed = {}
        regexes = {}
        ctx['flags'] = cls.flags
        for st in list(unprocessed):
            self.state(unprocessed, processed, st, regexes, ctx)
        pats = [None] * len(regexes)
        for (pat, fl), i in regexes.items():
            pats[i] = [pat, fl] if fl else [pat]
        return {'regexes': pats, 'states': processed}


def is_surrogate(ch):
    return 0xD800 <= ord(ch) <= 0xDFFF


def strip_surrogates(pattern):
    """Remove lone UTF-16 surrogates (only found in Unicode-category classes).

    They cannot occur in UTF-8 text, so clamping class ranges around them and
    dropping single ones keeps the match set identical for valid input.
    """
    if not any(is_surrogate(c) for c in pattern):
        return pattern
    out = []
    i, n = 0, len(pattern)
    while i < n:
        c = pattern[i]
        if i + 2 < n and pattern[i + 1] == '-' and (is_surrogate(c) or is_surrogate(pattern[i + 2])):
            lo, hi = c, pattern[i + 2]
            if is_surrogate(lo):
                lo = ''
            if is_surrogate(hi):
                hi = '퟿'
            if ord(lo) <= ord(hi):
                out.append(f'{lo}-{hi}')
            i += 3
            continue
        if not is_surrogate(c):
            out.append(c)
        i += 1
    return ''.join(out)


def closure_vars(fn):
    code = getattr(fn, '__code__', None)
    if code is None or not fn.__closure__:
        return {}
    out = {}
    for name, cell in zip(code.co_freevars, fn.__closure__):
        try:
            out[name] = cell.cell_contents
        except ValueError:
            pass
    return out


def jsonable(v):
    if v is None or isinstance(v, (bool, int, float, str)):
        return v
    if isinstance(v, (list, tuple)):
        return [jsonable(x) for x in v]
    if isinstance(v, dict):
        return {str(k): jsonable(x) for k, x in v.items()}
    if isinstance(v, re.Pattern):
        return {'regex': v.pattern, 'flags': flag_str(v.flags)}
    if type(v).__name__ == '_TokenType':
        return tok_name(v)
    if isinstance(v, type):
        return {'class': v.__name__}
    return {'repr': repr(v)}


def code_names(code, out):
    out.update(code.co_names)
    for c in code.co_consts:
        if hasattr(c, 'co_names'):
            code_names(c, out)
    return out


def is_const(v):
    """Data a Rust hook may need: string collections, maps of them, regexes, scalars."""
    if type(v).__name__ == '_TokenType':
        return False
    if isinstance(v, (str, int, float, bool)) or isinstance(v, re.Pattern):
        return True
    if isinstance(v, (list, tuple, set, frozenset)):
        return all(isinstance(x, (str, int)) or (isinstance(x, (list, tuple)) and all(isinstance(y, str) for y in x)) for x in v)
    if isinstance(v, dict):
        return all(isinstance(k, str) and is_const(x) for k, x in v.items())
    return False


def const_json(v):
    if isinstance(v, (set, frozenset)):
        return sorted(v)
    if isinstance(v, re.Pattern):
        return {'regex': strip_surrogates(v.pattern), 'flags': flag_str(v.flags)}
    if isinstance(v, (list, tuple)):
        return [const_json(x) for x in v]
    if isinstance(v, dict):
        return {k: const_json(x) for k, x in v.items()}
    return v


def hook_consts(inst, functions, plexer):
    """Constants referenced by Python hook code, resolved like the code would."""
    names = set()
    for fn in functions:
        code = getattr(fn, '__code__', None)
        if code is not None:
            code_names(code, names)
    import importlib
    import pkgutil
    import pygments.lexers as pl
    builtin_mods = [importlib.import_module(f'pygments.lexers.{m.name}') for m in pkgutil.iter_modules(pl.__path__) if m.name.endswith('builtins')]
    module = sys.modules[type(inst).__module__]
    base = set(dir(plexer.RegexLexer)) | set(dir(plexer.Lexer))
    out = {}
    for n in sorted(names):
        if n in base or n.startswith('__'):
            continue
        for scope in [inst.__dict__, type(inst), module] + builtin_mods:
            v = scope.get(n, None) if isinstance(scope, dict) else getattr(scope, n, None)
            if v is not None and is_const(v):
                out[n] = const_json(v)
                break
    return out


def hook_functions(cls, plexer):
    """Python functions that need a Rust port for this lexer (methods below the Pygments bases)."""
    bases = (plexer.Lexer, plexer.RegexLexer, plexer.ExtendedRegexLexer, plexer.DelegatingLexer, object)
    fns = []
    for c in cls.__mro__:
        if c in bases:
            continue
        for v in c.__dict__.values():
            if isinstance(v, (staticmethod, classmethod)):
                v = v.__func__
            if inspect.isfunction(v):
                fns.append(closure_vars(v).get('f', v))
    return fns


def defining_class(cls, attr):
    for c in cls.__mro__:
        if attr in c.__dict__:
            return c
    return None


def glob_regex(glob):
    # fnmatch.translate gives Python-syntax regex; Oniguruma's Python syntax accepts it.
    return fnmatch.translate(glob)


# -- analyse_text --------------------------------------------------------

class Unsupported(Exception):
    pass


class AnalyseConverter:
    """Convert the common shapes of analyse_text into a small heuristic IR.

    Supported: a sequence of `if COND: return NUM` / `return NUM` / `return COND`
    statements where COND is built from shebang_matches, doctype_matches,
    html_doctype_matches, looks_like_xml, re.search/re.match/compiled.search,
    `'lit' in text`, text.startswith(...), and/or/not. Anything else raises
    Unsupported and the lexer's analyse_text is listed for hand-porting.
    """

    def __init__(self, cls, module):
        self.cls = cls
        self.module = module

    def resolve(self, node):
        """Evaluate a constant-ish expression (string, regex) in the module/class scope."""
        src = ast.unparse(node)
        scope = dict(vars(self.module))
        scope.update({k: v for k, v in vars(self.cls).items()})
        try:
            return eval(src, scope)  # noqa: S307 - dev tool on a trusted, pinned checkout
        except Exception as err:
            raise Unsupported(f'cannot evaluate {src}: {err}') from err

    def flags_of(self, call, idx):
        if len(call.args) > idx:
            return flag_str(self.resolve(call.args[idx]))
        for kw in call.keywords:
            if kw.arg == 'flags':
                return flag_str(self.resolve(kw.value))
        return ''

    def is_text(self, node):
        return isinstance(node, ast.Name) and node.id == 'text'

    def cond(self, n):
        if isinstance(n, ast.BoolOp):
            op = 'and' if isinstance(n.op, ast.And) else 'or'
            return {op: [self.cond(v) for v in n.values]}
        if isinstance(n, ast.UnaryOp) and isinstance(n.op, ast.Not):
            return {'not': self.cond(n.operand)}
        if isinstance(n, ast.Compare) and len(n.ops) == 1 and isinstance(n.ops[0], (ast.In, ast.NotIn)):
            lit = self.resolve(n.left)
            if not (isinstance(lit, str) and self.is_text(n.comparators[0])):
                raise Unsupported('in-test on non-text')
            c = {'contains': lit}
            return {'not': c} if isinstance(n.ops[0], ast.NotIn) else c
        if isinstance(n, ast.Call):
            f = n.func
            fname = ast.unparse(f)
            if fname in ('shebang_matches', 'doctype_matches') and self.is_text(n.args[0]):
                return {fname.split('_')[0]: self.resolve(n.args[1])}
            if fname == 'html_doctype_matches' and self.is_text(n.args[0]):
                return {'html_doctype': True}
            if fname == 'looks_like_xml' and self.is_text(n.args[0]):
                return {'xml': True}
            if fname in ('re.search', 're.match') and len(n.args) >= 2 and self.is_text(n.args[1]):
                pat = self.resolve(n.args[0])
                pat = pat.pattern if isinstance(pat, re.Pattern) else pat
                return {fname[3:]: pat, 'flags': self.flags_of(n, 2)}
            if isinstance(f, ast.Attribute) and f.attr in ('search', 'match') and n.args and self.is_text(n.args[0]):
                obj = self.resolve(f.value)
                if isinstance(obj, re.Pattern):
                    return {f.attr: obj.pattern, 'flags': flag_str(obj.flags)}
            if isinstance(f, ast.Attribute) and f.attr in ('startswith', 'endswith') and self.is_text(f.value):
                lit = self.resolve(n.args[0])
                lits = list(lit) if isinstance(lit, tuple) else [lit]
                return {'or': [{f.attr: s} for s in lits]}
            if isinstance(f, ast.Attribute) and f.attr in ('startswith',) and ast.unparse(f.value) in ('text.lstrip()', 'text.strip()'):
                lit = self.resolve(n.args[0])
                lits = list(lit) if isinstance(lit, tuple) else [lit]
                return {'or': [{'lstrip_startswith': s} for s in lits]}
        raise Unsupported(f'condition {ast.unparse(n)}')

    def returns_one(self, n):
        """Condition under which `return n` scores 1.0. `make_analysator` floats the
        returned value: a bool gives 0/1, but an `re.Match` raises and scores 0, and
        `and`/`or` return one of their operands."""
        if isinstance(n, ast.BoolOp):
            if isinstance(n.op, ast.And):
                return {'and': [self.cond(v) for v in n.values[:-1]] + [self.returns_one(n.values[-1])]}
            first, rest = n.values[0], n.values[1:]
            other = self.returns_one(ast.BoolOp(op=ast.Or(), values=rest)) if len(rest) > 1 else self.returns_one(rest[0])
            return {'or': [{'and': [self.cond(first), self.returns_one(first)]},
                           {'and': [{'not': self.cond(first)}, other]}]}
        c = self.cond(n)
        if 'search' in c or 'match' in c:
            return {'or': []}  # a Match object scores 0
        return c

    def score(self, n):
        if n is None:
            return 0.0
        v = self.resolve(n)
        if isinstance(v, bool):
            return 1.0 if v else 0.0
        if isinstance(v, (int, float)):
            return float(v)
        raise Unsupported(f'score {ast.unparse(n)}')

    def body(self, stmts):
        out = []
        for s in stmts:
            if isinstance(s, ast.Expr) and isinstance(s.value, ast.Constant):
                continue  # docstring
            if isinstance(s, ast.If) and not s.orelse and len(s.body) == 1 and isinstance(s.body[0], ast.Return):
                out.append({'if': self.cond(s.test), 'then': self.score(s.body[0].value)})
                continue
            if isinstance(s, ast.If) and len(s.body) == 1 and isinstance(s.body[0], ast.Return) and len(s.orelse) == 1 and isinstance(s.orelse[0], ast.Return):
                out.append({'if': self.cond(s.test), 'then': self.score(s.body[0].value)})
                out.append({'return': self.score(s.orelse[0].value)})
                continue
            if isinstance(s, ast.Return):
                if s.value is None:
                    out.append({'return': 0.0})
                    continue
                try:
                    out.append({'return': self.score(s.value)})
                except Unsupported:
                    out.append({'if': self.returns_one(s.value), 'then': 1.0})
                    out.append({'return': 0.0})
                continue
            raise Unsupported(f'statement {ast.unparse(s).splitlines()[0]}')
        return out

    def convert(self, fn):
        src = textwrap.dedent(inspect.getsource(fn))
        tree = ast.parse(src)
        fdef = tree.body[0]
        if not isinstance(fdef, ast.FunctionDef):
            raise Unsupported('not a def')
        return self.body(fdef.body)


def analyse_text_of(cls, module, plexer):
    """Return (ir, None) when converted, (None, reason) when it needs hand-porting, (None, None) if absent."""
    owner = defining_class(cls, 'analyse_text')
    if owner in (plexer.Lexer, plexer.RegexLexer, plexer.ExtendedRegexLexer, plexer.DelegatingLexer):
        return None, None
    fn = owner.__dict__['analyse_text']
    if isinstance(fn, staticmethod):
        fn = fn.__func__
    fn = closure_vars(fn).get('f', fn)  # unwrap pygments.util.make_analysator
    try:
        return AnalyseConverter(owner, sys.modules[owner.__module__]).convert(fn), None
    except Exception as err:  # any failure means: hand-port it
        return None, f'{owner.__name__}: {err}'


# -- main ----------------------------------------------------------------

def rust_ident(name):
    s = re.sub(r'[^0-9A-Za-z]+', '_', name).strip('_').upper()
    return s or 'TOKEN'


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--pygments', required=True)
    args = ap.parse_args()
    pygments, plexer, plexers, pstyles, ptoken = load_pygments(args.pygments)
    from pygments.lexers._mapping import LEXERS

    out_dir = ROOT / 'data' / 'lexers'
    out_dir.mkdir(parents=True, exist_ok=True)
    for old in out_dir.glob('*.json'):
        old.unlink()

    conv = Converter(plexer, ptoken)
    index = []
    report = {}
    analyse = {}
    # Unregistered helper lexers that hand-ported code reaches by class name.
    # They get no aliases, filenames or MIME types, so lookups never return them.
    helpers = {'PsqlRegexLexer': 'pygments.lexers.sql'}
    entries = {k: v for k, v in LEXERS.items()}
    for clsname, modname in helpers.items():
        hcls = getattr(importlib.import_module(modname), clsname)
        entries[clsname] = (modname, hcls.name, (), (), ())
    for clsname in sorted(entries):
        modname, name, aliases, filenames, mimetypes = entries[clsname]
        cls = (getattr(importlib.import_module(modname), clsname) if clsname in helpers
               else plexers.find_lexer_class(name))
        assert cls.__name__ == clsname, (cls, clsname)
        module = sys.modules[cls.__module__]
        inst = cls()
        entry = {
            'class': clsname,
            'name': name,
            'module': modname.rsplit('.', 1)[-1],
            'aliases': list(aliases),
            'filenames': [[g, glob_regex(g)] for g in filenames],
            'alias_filenames': [[g, glob_regex(g)] for g in cls.alias_filenames],
            'mimetypes': list(mimetypes),
            'priority': float(cls.priority),
            'url': getattr(cls, 'url', None),
        }
        opts = {'stripnl': inst.stripnl, 'stripall': inst.stripall, 'ensurenl': inst.ensurenl, 'tabsize': inst.tabsize}
        if opts != {'stripnl': True, 'stripall': False, 'ensurenl': True, 'tabsize': 0}:
            entry['options'] = opts
        needs = []
        ctx = {'callbacks': set(), 'callback_fns': [], 'tmp': 0}
        gtu_owner = defining_class(cls, 'get_tokens_unprocessed')
        if isinstance(inst, plexer.DelegatingLexer) and gtu_owner is plexer.DelegatingLexer:
            entry['kind'] = 'delegating'
            entry['root'] = type(inst.root_lexer).__name__
            entry['language'] = type(inst.language_lexer).__name__
            entry['needle'] = tok_name(inst.needle)
        elif isinstance(inst, plexer.RegexLexer):
            entry['kind'] = 'regex'
            entry['extended'] = isinstance(inst, plexer.ExtendedRegexLexer)
            entry.update(conv.regex_lexer(cls, inst, ctx))
            if 'variant' in ctx:
                entry['variant'] = ctx['variant']
            if gtu_owner not in (plexer.RegexLexer, plexer.ExtendedRegexLexer):
                entry['override'] = gtu_owner.__name__
                default = inspect.signature(gtu_owner.get_tokens_unprocessed).parameters.get('stack')
                if default is not None and tuple(default.default) != ('root',):
                    # Only changes the starting stack (Inform6TemplateLexer); the engine handles it.
                    entry['initial_stack'] = list(default.default)
                else:
                    needs.append(f'get_tokens_unprocessed override in {gtu_owner.__name__}')
            for cb in sorted(ctx['callbacks']):
                needs.append(f'callback {cb}')
        else:
            entry['kind'] = 'custom'
            entry['override'] = gtu_owner.__name__
            needs.append(f'custom lexer ({gtu_owner.__name__}.get_tokens_unprocessed)')
        if needs:
            fns = hook_functions(cls, plexer)
            for cb in ctx['callback_fns']:
                fns.append(cb)
            consts = hook_consts(inst, fns, plexer)
            if consts:
                entry['consts'] = consts
        ir, reason = analyse_text_of(cls, module, plexer)
        if ir is not None:
            entry['analyse'] = ir
        elif reason:
            entry['analyse_custom'] = defining_class(cls, 'analyse_text').__name__
            analyse[clsname] = {'custom': entry['analyse_custom']}
            needs.append(f'analyse_text ({reason})')
        if 'analyse' in entry:
            analyse[clsname] = entry.pop('analyse')
        (out_dir / f'{clsname}.json').write_text(json.dumps(entry, ensure_ascii=False, separators=(',', ':')), encoding='utf-8')
        index.append(entry)
        report[clsname] = needs

    write_tokens(ptoken)
    write_styles(pstyles, ptoken)
    write_registry(index)
    (ROOT / 'data' / 'analyse.json').write_text(json.dumps(analyse, ensure_ascii=False, separators=(',', ':'), sort_keys=True), encoding='utf-8')
    (ROOT / 'data' / 'report.json').write_text(json.dumps(report, indent=1, sort_keys=True), encoding='utf-8')
    (ROOT / 'PYGMENTS_VERSION').write_text(pygments.__version__ + '\n', encoding='utf-8')
    auto = sum(1 for n in report.values() if not n)
    print(f'{len(index)} lexers, {auto} need no Rust code, {len(index) - auto} need hand-ported pieces')


def all_tokens(ptoken):
    # Created lazily inside lexer code at lex time (found by grepping pygments/lexers), so the import-time tree lacks it.
    ptoken.Generic.Prompt.VirtualEnv  # noqa: B018
    ptoken.Name.Builtin.SchemeFunction  # noqa: B018 (LilyPondLexer)
    ptoken.Name.Builtin.SchemeBuiltin  # noqa: B018
    ptoken.Text.Whitespace.Error  # noqa: B018 (YamlLexer.save_indent)
    seen = []

    def walk(t):
        seen.append(t)
        for sub in sorted(t.subtypes, key=str):
            walk(sub)
    walk(ptoken.Token)
    return seen


def write_tokens(ptoken):
    toks = all_tokens(ptoken)
    index = {t: i for i, t in enumerate(toks)}
    lines = [
        '// @generated by tools/convert.py from Pygments. Do not edit.',
        '#![allow(missing_docs)]',
        'use crate::token::TokenType;',
        '',
        '/// `(full name, parent index, short CSS class)`; index 0 is `Token`.',
        f'pub(crate) static TOKENS: [(&str, u16, &str); {len(toks)}] = [',
    ]
    for t in toks:
        parent = index[t.parent] if t.parent is not None else 0
        short = ptoken.STANDARD_TYPES.get(t, '')
        lines.append(f'    ({json.dumps(str(t))}, {parent}, {json.dumps(short)}),')
    lines.append('];')
    lines.append('')
    lines.append('impl TokenType {')
    used = set()
    for t, i in index.items():
        ident = rust_ident(str(t)[6:]) if t is not ptoken.Token else 'TOKEN'
        if ident in used:
            continue
        used.add(ident)
        lines.append(f'    pub const {ident}: TokenType = TokenType({i});')
    lines.append('}')
    (ROOT / 'src' / 'generated').mkdir(parents=True, exist_ok=True)
    (ROOT / 'src' / 'generated' / 'tokens.rs').write_text('\n'.join(lines) + '\n', encoding='utf-8')


def write_styles(pstyles, ptoken):
    styles = {}
    for name in sorted(pstyles.get_all_styles()):
        st = pstyles.get_style_by_name(name)
        entries = {}
        for tok, (color, bold, italic, underline, bgcolor, border, roman, sans, mono) in st._styles.items():
            e = {}
            if color: e['color'] = color
            if bgcolor: e['bg'] = bgcolor
            if border: e['border'] = border
            if bold: e['bold'] = True
            if italic: e['italic'] = True
            if underline: e['underline'] = True
            if roman is not None and roman: e['roman'] = True
            if sans: e['sans'] = True
            if mono: e['mono'] = True
            entries[str(tok)] = e
        styles[name] = {
            'name': getattr(st, 'name', name),
            'background': st.background_color,
            'highlight': st.highlight_color,
            'line_number_color': st.line_number_color,
            'line_number_background': st.line_number_background_color,
            'tokens': entries,
        }
    (ROOT / 'data' / 'styles.json').write_text(json.dumps(styles, indent=0, sort_keys=True), encoding='utf-8')


def write_registry(index):
    lines = [
        '// @generated by tools/convert.py from Pygments. Do not edit.',
        'use crate::registry::LexerInfo;',
        '',
        f'pub(crate) static LEXERS: [LexerInfo; {len(index)}] = [',
    ]
    for e in index:
        lines.append('    LexerInfo {')
        lines.append(f'        class: {json.dumps(e["class"])},')
        lines.append(f'        name: {json.dumps(e["name"], ensure_ascii=False)},')
        lines.append(f'        aliases: &{json.dumps(e["aliases"], ensure_ascii=False)},')
        lines.append(f'        filenames: &{json.dumps([g for g, _ in e["filenames"]], ensure_ascii=False)},')
        lines.append(f'        mimetypes: &{json.dumps(e["mimetypes"])},')
        lines.append(f'        priority: {e["priority"]!r},')
        o = e.get('options', {})
        lines.append(f'        stripnl: {str(o.get("stripnl", True)).lower()},')
        lines.append(f'        stripall: {str(o.get("stripall", False)).lower()},')
        lines.append(f'        ensurenl: {str(o.get("ensurenl", True)).lower()},')
        lines.append(f'        tabsize: {int(o.get("tabsize", 0))},')
        lines.append(f'        data: include_str!("../../data/lexers/{e["class"]}.json"),')
        lines.append('    },')
    lines.append('];')
    (ROOT / 'src' / 'generated' / 'registry.rs').write_text('\n'.join(lines) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
