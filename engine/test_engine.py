##   AlgebraKing ##
##
##   File:         [test_engine.py]
##
##   Author(s):    ['Pedro H.F Matias']
##   Site(s):      ['https://github.com/phfmatias']
##   Email(s):     ['pedrohfmatias@gmail.com']
##   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
##   Date:         ['27.10.2026']
##   Version:      ['0.0.1']
##   Status:       ['Development']
##   Language:     ['Python']
##   Description:  ['Unit tests that talk the real JSON-lines protocol to engine.py as a subprocess.']
##   Usage:        ['python3 -m unittest engine/test_engine.py -q']

import json
import os
import re
import subprocess
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ENGINE = os.path.join(HERE, 'engine.py')

CONSTS = {'Pi': 'Pi', 'I': 'I', 'infinity': 'infinity', 'Catalan': 'Catalan', 'undefined': 'undefined'}


# ----------------------------------------------------------------------------
# Tiny Maple-like parser used only to write test inputs as readable strings
# ----------------------------------------------------------------------------

class P(object):
    """Parse a Maple-flavoured string into an EXPR tree (just enough syntax for the tests)."""

    TOKEN = re.compile(r'\s*(?:(\d+\.(?!\.)\d*(?:[eE][-+]?\d+)?|\.\d+|\d+)|(_?[A-Za-z_][A-Za-z_0-9]*)|(\.\.|<>|<=|>=|:=|->|\*\*|[-+*/^=<>,()\[\]{}|$]))')

    def __init__(self, text):
        self.toks = []
        pos = 0
        text = text.strip()
        while pos < len(text):
            m = self.TOKEN.match(text, pos)
            if not m:
                raise ValueError('bad token at {!r}'.format(text[pos:]))
            pos = m.end()
            if m.group(1) is not None:
                self.toks.append(('num', m.group(1)))
            elif m.group(2) is not None:
                self.toks.append(('id', m.group(2)))
            else:
                self.toks.append(('op', m.group(3)))
        self.i = 0

    def peek(self):
        return self.toks[self.i] if self.i < len(self.toks) else (None, None)

    def take(self, val=None):
        t = self.toks[self.i]
        if val is not None and t[1] != val:
            raise ValueError('expected {} got {}'.format(val, t))
        self.i += 1
        return t

    def isOp(self, *ops):
        k, v = self.peek()
        return k == 'op' and v in ops

    def parseAll(self):
        e = self.seqExpr()
        if self.i != len(self.toks):
            raise ValueError('trailing tokens {}'.format(self.toks[self.i:]))
        return e

    def seqExpr(self):
        items = [self.rel()]
        while self.isOp(','):
            self.take(',')
            items.append(self.rel())
        return items[0] if len(items) == 1 else {'seq': items}

    def rel(self):
        left = self.rng()
        while self.isOp('=', '<>', '<', '<=', '>', '>='):
            op = self.take()[1]
            right = self.rng()
            if op == '=':
                left = {'eq': [left, right]}
            elif op == '<>':
                left = {'neq': [left, right]}
            elif op == '<':
                left = {'lt': [left, right]}
            elif op == '<=':
                left = {'le': [left, right]}
            elif op == '>':
                left = {'lt': [right, left]}
            else:
                left = {'le': [right, left]}
        return left

    def rng(self):
        left = self.add()
        if self.isOp('..'):
            self.take('..')
            return {'range': [left, self.add()]}
        return left

    def add(self):
        terms = [self.mul()]
        while self.isOp('+', '-'):
            op = self.take()[1]
            t = self.mul()
            terms.append(t if op == '+' else {'mul': [{'int': '-1'}, t]})
        return terms[0] if len(terms) == 1 else {'add': terms}

    def mul(self):
        factors = [self.unary()]
        while self.isOp('*', '/'):
            op = self.take()[1]
            f = self.unary()
            factors.append(f if op == '*' else {'pow': [f, {'int': '-1'}]})
        return factors[0] if len(factors) == 1 else {'mul': factors}

    def unary(self):
        if self.isOp('-'):
            self.take('-')
            return {'mul': [{'int': '-1'}, self.unary()]}
        if self.isOp('+'):
            self.take('+')
            return self.unary()
        return self.power()

    def power(self):
        base = self.postfix()
        if self.isOp('^', '**'):
            self.take()
            return {'pow': [base, self.unary()]}
        return base

    def postfix(self):
        k, v = self.peek()
        if k == 'num':
            self.take()
            if '.' in v or 'e' in v.lower():
                digits = len(re.sub(r'[^0-9]', '', v.lower().split('e')[0]).lstrip('0')) or 1
                return {'float': v, 'digits': max(digits, 10)}
            return {'int': v}
        if k == 'id':
            self.take()
            if self.isOp('('):
                self.take('(')
                args = []
                if not self.isOp(')'):
                    args.append(self.rel())
                    while self.isOp(','):
                        self.take(',')
                        args.append(self.rel())
                self.take(')')
                return {'fn': v, 'args': args}
            if v in CONSTS:
                return {'const': CONSTS[v]}
            return {'sym': v}
        if k == 'op' and v == '(':
            self.take('(')
            e = self.seqExpr()
            self.take(')')
            return e
        if k == 'op' and v in ('[', '{'):
            close = ']' if v == '[' else '}'
            self.take()
            items = []
            if not self.isOp(close):
                items.append(self.rel())
                while self.isOp(','):
                    self.take(',')
                    items.append(self.rel())
            self.take(close)
            return {'list' if v == '[' else 'set': items}
        raise ValueError('unexpected token {}'.format((k, v)))


def X(text):
    """Maple-like string -> EXPR."""
    return P(text).parseAll() if isinstance(text, str) else text


def MAT(rows):
    return {'matrix': [[X(c) for c in r] for r in rows]}


def VEC(items, orient='column'):
    return {'vector': [X(c) for c in items], 'orient': orient}


# ----------------------------------------------------------------------------
# EXPR -> compact readable string (for assertions), close to Maple 1-D output
# ----------------------------------------------------------------------------

def negate(e):
    """If e is a negative term return the positive counterpart, else None."""
    if not isinstance(e, dict):
        return None
    if 'int' in e and int(e['int']) < 0:
        return {'int': str(-int(e['int']))}
    if 'rat' in e and int(e['rat'][0]) < 0:
        return {'rat': [str(-int(e['rat'][0])), e['rat'][1]]}
    if 'float' in e and e['float'].startswith('-'):
        return {'float': e['float'][1:], 'digits': e.get('digits')}
    if 'mul' in e and e['mul']:
        nf = negate(e['mul'][0])
        if nf is not None:
            rest = e['mul'][1:]
            if 'int' in nf and nf['int'] == '1':
                if not rest:
                    return nf
                return rest[0] if len(rest) == 1 else {'mul': rest}
            return {'mul': [nf] + rest}
    return None


def negExp(e):
    """Positive version of a negative exponent, else None."""
    return negate(e)


def S(e, ctx=0):
    """Render an EXPR as a compact Maple-like string. ctx: 0 top, 1 add term, 2 mul factor, 3 pow base, 4 pow exponent."""
    if not isinstance(e, dict):
        return repr(e)
    if 'int' in e:
        s = e['int']
        return '({})'.format(s) if s.startswith('-') and ctx >= 3 else s
    if 'rat' in e:
        s = '{}/{}'.format(*e['rat'])
        return '({})'.format(s) if ctx >= 2 else s
    if 'float' in e:
        return '({})'.format(e['float']) if e['float'].startswith('-') and ctx >= 3 else e['float']
    if 'sym' in e:
        return e['sym']
    if 'idx' in e:
        return '{}[{}]'.format(S(e['idx'][0]), ','.join(S(i) for i in e['idx'][1]))
    if 'str' in e:
        return '"{}"'.format(e['str'])
    if 'bool' in e:
        return 'true' if e['bool'] else 'false'
    if 'fail' in e:
        return 'FAIL'
    if 'const' in e:
        return e['const']
    if 'add' in e:
        out = ''
        for i, t in enumerate(e['add']):
            n = negate(t)
            if n is not None:
                out += '-' + S(n, 1) if i == 0 or True else ''
            else:
                out += ('+' if i else '') + S(t, 1)
        return '({})'.format(out) if ctx >= 2 else out
    if 'mul' in e:
        nums, dens = [], []
        sign = ''
        facs = list(e['mul'])
        neg = negate(facs[0]) if facs else None
        if neg is not None:
            sign = '-'
            facs[0] = neg
            if 'int' in neg and neg['int'] == '1':
                facs = facs[1:]
        for f in facs:
            if 'pow' in f:
                ne = negExp(f['pow'][1])
                if ne is not None:
                    dens.append(f['pow'][0] if ne.get('int') == '1' else {'pow': [f['pow'][0], ne]})
                    continue
            if 'rat' in f:
                p, q = f['rat']
                if p != '1':
                    nums.append({'int': p})
                dens.append({'int': q})
                continue
            nums.append(f)
        numS = '*'.join(S(n, 2) for n in nums) or '1'
        out = numS
        if dens:
            if len(dens) == 1:
                d = S(dens[0], 1)
                out += '/' + ('({})'.format(d) if 'mul' in dens[0] or 'add' in dens[0] else d)
            else:
                out += '/(' + '*'.join(S(x, 2) for x in dens) + ')'
        out = sign + out
        return '({})'.format(out) if ctx >= 3 else out
    if 'pow' in e:
        b, x = e['pow']
        if 'rat' in x and x['rat'] == ['1', '2']:
            return 'sqrt({})'.format(S(b))
        ne = negExp(x)
        if ne is not None:
            inner = b if ne.get('int') == '1' else {'pow': [b, ne]}
            return '1/' + S(inner, 3)
        s = '{}^{}'.format(S(b, 3), S(x, 4))
        return '({})'.format(s) if ctx >= 3 else s
    if 'fn' in e:
        head = S(e['fn']) if isinstance(e['fn'], dict) else e['fn']
        return '{}({})'.format(head, ','.join(S(a) for a in e.get('args', [])))
    if 'fnidx' in e:
        n, idx, args = e['fnidx']
        return '{}[{}]({})'.format(n, ','.join(S(i) for i in idx), ','.join(S(a) for a in args))
    for k, sym in (('eq', '='), ('neq', '<>'), ('lt', '<'), ('le', '<=')):
        if k in e:
            return '{}{}{}'.format(S(e[k][0]), sym, S(e[k][1]))
    if 'and' in e:
        return ' and '.join(S(t) for t in e['and'])
    if 'or' in e:
        return ' or '.join(S(t) for t in e['or'])
    if 'not' in e:
        return 'not ' + S(e['not'])
    if 'list' in e:
        return '[' + ','.join(S(t) for t in e['list']) + ']'
    if 'set' in e:
        return '{' + ','.join(S(t) for t in e['set']) + '}'
    if 'seq' in e:
        return ','.join(S(t) for t in e['seq'])
    if 'range' in e:
        return '{}..{}'.format(S(e['range'][0]), S(e['range'][1]))
    if 'matrix' in e:
        return 'Matrix[' + ','.join('[' + ','.join(S(c) for c in r) + ']' for r in e['matrix']) + ']'
    if 'vector' in e:
        return ('Vector[' if e.get('orient') == 'column' else 'RowVector[') + ','.join(S(c) for c in e['vector']) + ']'
    if 'proc' in e:
        return '({})->{}'.format(','.join(e['proc']['params']), S(e['proc']['body']))
    return repr(e)


class EngineProc(object):
    """The engine as a subprocess speaking JSON lines."""

    def __init__(self):
        self.proc = subprocess.Popen([sys.executable, '-u', ENGINE], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        self.n = 0

    def raw(self, line):
        self.proc.stdin.write(line + '\n')
        self.proc.stdin.flush()
        return self.proc.stdout.readline()

    def send(self, req):
        self.n += 1
        req = dict(req)
        req['id'] = self.n
        out = self.raw(json.dumps(req))
        resp = json.loads(out)
        assert resp['id'] == self.n, resp
        return resp

    def close(self):
        try:
            self.proc.stdin.close()
            self.proc.wait(timeout=10)
        except (OSError, subprocess.TimeoutExpired):
            self.proc.kill()
        finally:
            self.proc.stdout.close()


_ENGINE = None


def engine():
    global _ENGINE
    if _ENGINE is None or _ENGINE.proc.poll() is not None:
        _ENGINE = EngineProc()
    return _ENGINE


def tearDownModule():
    if _ENGINE is not None:
        _ENGINE.close()


class Base(unittest.TestCase):
    def call(self, name, *args, digits=10, assume=None, idx=None):
        req = {'op': 'call', 'name': name, 'args': [X(a) for a in args],
               'opts': {'digits': digits, 'assume': [X(a) for a in (assume or [])]}}
        return engine().send(req)

    def ok(self, name, *args, **kw):
        r = self.call(name, *args, **kw)
        self.assertTrue(r['ok'], r)
        return r['result']

    def s(self, name, *args, **kw):
        """Result of a call rendered as a string."""
        return S(self.ok(name, *args, **kw))

    def eqv(self, name, *args, expect, **kw):
        """Result equals the expected Maple string up to ordering of terms (checked with the engine's verify)."""
        res = self.ok(name, *args, **kw)
        r = engine().send({'op': 'verify', 'lhs': res, 'rhs': X(expect), 'opts': {'digits': 10, 'assume': []}})
        self.assertEqual(r['result']['verdict'], 'equal', (S(res), expect))

    def err(self, name, *args, **kw):
        r = self.call(name, *args, **kw)
        self.assertFalse(r['ok'], r)
        return r['error']

    def verify(self, lhs, rhs, digits=10, assume=None):
        req = {'op': 'verify', 'lhs': X(lhs), 'rhs': X(rhs),
               'opts': {'digits': digits, 'assume': [X(a) for a in (assume or [])]}}
        r = engine().send(req)
        self.assertTrue(r['ok'], r)
        return r['result']


def TYPE(name, typ):
    return {'fn': 'type', 'args': [{'sym': name}, {'sym': typ}]}


class TestProtocol(Base):
    def test_ping(self):
        r = engine().send({'op': 'ping'})
        self.assertTrue(r['ok'])

    def test_id_is_echoed(self):
        out = json.loads(engine().raw(json.dumps({'id': 'abc', 'op': 'ping'})))
        self.assertEqual(out['id'], 'abc')

    def test_invalid_json_does_not_kill_the_process(self):
        out = json.loads(engine().raw('{not json'))
        self.assertFalse(out['ok'])
        self.assertTrue(engine().send({'op': 'ping'})['ok'])

    def test_non_object_request(self):
        out = json.loads(engine().raw('[1, 2, 3]'))
        self.assertFalse(out['ok'])
        self.assertTrue(engine().send({'op': 'ping'})['ok'])

    def test_unknown_op(self):
        r = engine().send({'op': 'frobnicate'})
        self.assertFalse(r['ok'])

    def test_unknown_function_stays_unevaluated(self):
        res = self.ok('myfunc', 'x', 'y+1')
        self.assertEqual(res['fn'], 'myfunc')
        self.assertEqual(S(res), 'myfunc(x,y+1)')

    def test_errors_use_maple_style(self):
        self.assertEqual(self.err('int', 'x'), '(in int) wrong number (or type) of arguments')
        self.assertTrue(self.err('ln', '0').startswith('(in ln) numeric exception'))

    def test_error_then_normal_call(self):
        self.err('int', 'x')
        self.assertEqual(self.s('expand', '(x+1)^2'), 'x^2+2*x+1')

    def test_missing_fields_do_not_crash(self):
        r = engine().send({'op': 'call'})
        self.assertIn('ok', r)
        r = engine().send({'op': 'verify'})
        self.assertFalse(r['ok'])
        self.assertTrue(engine().send({'op': 'ping'})['ok'])

    def test_one_line_per_request(self):
        for _ in range(3):
            line = engine().raw(json.dumps({'id': 1, 'op': 'call', 'name': 'expand',
                                            'args': [X('(a+b)^2')], 'opts': {}}))
            self.assertTrue(line.endswith('\n'))
            self.assertEqual(line.count('\n'), 1)

    def test_package_prefixed_names(self):
        self.assertEqual(self.s('LinearAlgebra:-Determinant', MAT([['1', '2'], ['3', '4']])), '-2')
        self.assertEqual(self.s('numtheory[phi]', '12'), '4')

    def test_stateless_digits_and_assume_travel_with_the_request(self):
        self.assertEqual(self.s('evalf', 'Pi', digits=5), '3.1416')
        self.assertEqual(self.s('evalf', 'Pi', digits=12), '3.14159265359')
        self.assertEqual(self.s('simplify', 'sqrt(x^2)', assume=['0<x']), 'x')
        self.assertEqual(self.s('simplify', 'sqrt(x^2)'), 'sqrt(x^2)')


class TestHygiene(unittest.TestCase):
    def test_stdout_only_has_json_and_stderr_is_quiet(self):
        proc = subprocess.Popen([sys.executable, '-u', ENGINE], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True)
        calls = [('integrate', 'int', ['sin(x)*exp(x)', 'x']), ('solve', 'solve', ['x^3-x-1=0', 'x']),
                 ('series', 'series', ['exp(sin(x))', 'x=0', '8']), ('bad', 'int', ['x'])]
        for i, (_, name, args) in enumerate(calls):
            req = {'id': i, 'op': 'call', 'name': name, 'args': [X(a) for a in args], 'opts': {'digits': 10}}
            proc.stdin.write(json.dumps(req) + '\n')
            proc.stdin.flush()
            line = proc.stdout.readline()
            self.assertEqual(json.loads(line)['id'], i)
        proc.stdin.close()
        rest = proc.stdout.read()
        err = proc.stderr.read()
        proc.wait(timeout=10)
        proc.stdout.close()
        proc.stderr.close()
        self.assertEqual(rest, '')
        self.assertEqual(err, '')


class TestFloats(Base):
    def test_default_ten_digits(self):
        r = self.ok('evalf', 'Pi')
        self.assertEqual(r, {'float': '3.141592654', 'digits': 10})

    def test_digits_option_in_request(self):
        r = self.ok('evalf', 'Pi', digits=30)
        self.assertEqual(r['digits'], 30)
        self.assertEqual(len(re.sub(r'\D', '', r['float'])), 30)
        self.assertEqual(r['float'], '3.14159265358979323846264338328')

    def test_evalf_second_argument(self):
        self.assertEqual(self.s('evalf', 'Pi', '5'), '3.1416')
        r = self.ok('evalf', 'sqrt(2)', '20')
        self.assertEqual(r['float'], '1.4142135623730950488')
        self.assertEqual(r['digits'], 20)

    def test_evalf_indexed_form(self):
        req = {'op': 'call', 'name': 'evalf', 'args': [X('Pi')], 'idx': [X('15')], 'opts': {'digits': 10}}
        r = engine().send(req)
        self.assertEqual(r['result']['float'], '3.14159265358979')
        self.assertEqual(r['result']['digits'], 15)

    def test_evalf_fnidx_inside_expression(self):
        node = {'fnidx': ['evalf', [X('12')], [X('Pi')]]}
        r = engine().send({'op': 'call', 'name': 'simplify', 'args': [node], 'opts': {'digits': 10}})
        self.assertEqual(r['result']['float'], '3.14159265359')

    def test_trailing_zeros_kept_after_evalf(self):
        self.assertEqual(self.s('evalf', '5/2'), '2.500000000')
        self.assertEqual(self.s('evalf', '1/4'), '0.2500000000')

    def test_integer_valued_float(self):
        self.assertEqual(self.s('evalf', '2'), '2.')

    def test_small_and_large_use_exponent(self):
        self.assertEqual(self.s('evalf', '1/70000000'), '1.428571429e-8')
        self.assertEqual(self.s('evalf', '10^20/7'), '1.428571429e19')

    def test_float_input_is_contagious(self):
        self.assertEqual(self.s('sin', '1.0'), '0.8414709848')
        self.assertEqual(self.s('BesselJ', '0', '1.0'), '0.7651976866')

    def test_float_literal_prints_back(self):
        self.assertEqual(self.s('expand', 'x*2.50'), '2.5*x')
        self.assertEqual(self.s('simplify', '0.5*x+0.25*x'), '0.75*x')
        self.assertEqual(self.s('subs', 'x=0.1', 'x*3'), '0.3')

    def test_float_input_with_node_digits(self):
        r = engine().send({'op': 'call', 'name': 'sin', 'args': [{'float': '1.0', 'digits': 10}],
                           'opts': {'digits': 30}})
        self.assertEqual(r['result']['digits'], 30)
        self.assertEqual(r['result']['float'], '0.84147098480789650665250232163')

    def test_complex_evalf(self):
        self.assertEqual(self.s('evalf', 'exp(I)'), '0.5403023059+0.8414709848*I')

    def test_digits_one(self):
        self.assertEqual(self.s('evalf', '1/7', digits=1), '0.1')

    def test_negative_float(self):
        self.assertEqual(self.s('evalf', '-Pi'), '-3.141592654')

    def test_evalf_maps_over_structures(self):
        self.assertEqual(self.s('evalf', '[Pi, 1/3]'), '[3.141592654,0.3333333333]')

    def test_solve_with_float_coefficients_prints_digits(self):
        self.assertEqual(self.s('solve', 'x^2=2.0', 'x'), '1.414213562,-1.414213562')
        self.assertEqual(self.s('solve', 'x^2=2.0', 'x', digits=20), '1.4142135623730950488,-1.4142135623730950488')

    def test_long_literal_keeps_its_digits(self):
        r = engine().send({'op': 'call', 'name': 'expand', 'args': [{'mul': [X('x'), {'float': '0.123456789012345678901', 'digits': 21}]}],
                           'opts': {'digits': 10}})
        self.assertEqual(S(r['result']), '0.123456789012345678901*x')

    def test_float_contagion_in_arithmetic(self):
        self.assertEqual(self.s('simplify', 'Pi*1.0'), '3.141592654')
        self.assertEqual(self.s('simplify', 'sqrt(2)+1.0'), '2.414213562')

    def test_evalf_of_inert_limit_and_sum(self):
        self.assertEqual(self.s('evalf', 'Limit(sin(x)/x, x=0)'), '1.')
        self.assertEqual(self.s('evalf', 'Sum(1/k^2,k=1..100)'), '1.634983900')
        self.assertEqual(self.s('evalf', 'Int(sin(x)/x, x=0..1)'), '0.9460830704')

    def test_evalf_symbolic_keeps_symbols(self):
        self.assertEqual(self.s('evalf', 'x/3+Pi*y'), '0.3333333333*x+3.141592654*y')


class TestArithmetic(Base):
    def test_simplify_trig(self):
        self.assertEqual(self.s('simplify', 'sin(x)^2+cos(x)^2'), '1')

    def test_simplify_with_trig_option(self):
        self.assertEqual(self.s('simplify', 'sin(x)^2+cos(x)^2', 'trig'), '1')

    def test_simplify_prefers_the_shorter_trig_form(self):
        self.assertEqual(self.s('simplify', '(sin(x)+cos(x))^2'), 'sin(2*x)+1')

    def test_piecewise_numeric_condition(self):
        self.assertEqual(self.s('piecewise', '-2<0', '2', '9'), '2')
        self.assertEqual(self.s('piecewise', '3<0', '2', '9'), '9')

    def test_simplify_euler_identity(self):
        self.assertEqual(self.s('simplify', 'F*cos(w*t)-(F*exp(-I*w*t)/2+F*exp(I*w*t)/2)'), '0')
        self.assertEqual(self.s('simplify', 'exp(I*x)'), 'exp(I*x)')

    def test_simplify_with_assume_option(self):
        self.assertEqual(self.s('simplify', 'sqrt(x^2*y^2)', 'assume=positive'), 'x*y')

    def test_simplify_rational_function(self):
        self.assertEqual(self.s('simplify', '(x^2-1)/(x-1)'), 'x+1')

    def test_simplify_maps_over_lists_and_equations(self):
        self.assertEqual(self.s('simplify', '[x+x, (x^2-1)/(x-1)]'), '[2*x,x+1]')
        self.assertEqual(self.s('simplify', 'x+x=y+y'), '2*x=2*y')

    def test_expand(self):
        self.assertEqual(self.s('expand', '(x+1)^3'), 'x^3+3*x^2+3*x+1')
        self.assertEqual(self.s('expand', '(a+b)*(a-b)'), 'a^2-b^2')

    def test_expand_trig_and_exp(self):
        self.assertEqual(self.s('expand', 'sin(2*x)'), '2*sin(x)*cos(x)')
        self.assertEqual(self.s('expand', 'exp(x+y)'), 'exp(x)*exp(y)')

    def test_expand_ln_of_numeric_factor(self):
        self.assertEqual(self.s('expand', 'ln(2*x)'), 'ln(x)+ln(2)')

    def test_factor(self):
        self.assertEqual(self.s('factor', 'x^2-1'), '(x-1)*(x+1)')
        self.assertEqual(self.s('factor', 'x^3-6*x^2+11*x-6'), '(x-3)*(x-2)*(x-1)')

    def test_factor_with_extension(self):
        self.assertEqual(self.s('factor', 'x^2+1', 'I'), '(x-I)*(x+I)')
        self.assertEqual(self.s('factor', 'x^2-2', 'sqrt(2)'), '(x-sqrt(2))*(x+sqrt(2))')
        self.assertEqual(self.s('factor', 'x^2+1'), 'x^2+1')

    def test_normal(self):
        self.assertEqual(self.s('normal', 'x/(x+1)+1/x'), '(x^2+x+1)/(x*(x+1))')
        self.assertEqual(self.s('normal', '(x^2-1)/(x-1)'), 'x+1')

    def test_combine(self):
        self.assertEqual(self.s('combine', 'sin(x)*cos(x)', 'trig'), 'sin(2*x)/2')
        self.assertEqual(self.s('combine', 'exp(x)*exp(y)', 'exp'), 'exp(x+y)')
        self.assertEqual(self.s('combine', 'x^2*x^3', 'power'), 'x^5')

    def test_combine_ln_needs_assumptions(self):
        self.assertEqual(self.s('combine', 'ln(x)+ln(y)', 'ln'), 'ln(x)+ln(y)')
        self.assertEqual(self.s('combine', 'ln(x)+ln(y)', 'ln', assume=['0<x', '0<y']), 'ln(x*y)')

    def test_collect(self):
        self.assertEqual(self.s('collect', 'a*x+b*x+x^2*c+d', 'x'), 'c*x^2+(a+b)*x+d')
        self.assertEqual(self.s('collect', 'x*y+x*z+y^2', 'x'), '(y+z)*x+y^2')

    def test_sort(self):
        self.assertEqual(self.s('sort', '[3,1,2]'), '[1,2,3]')

    def test_rationalize_and_radnormal(self):
        self.eqv('rationalize', '1/(sqrt(2)+1)', expect='sqrt(2)-1')
        self.eqv('radnormal', '1/(sqrt(2)+1)', expect='sqrt(2)-1')
        self.assertEqual(self.s('rationalize', '1/(sqrt(2)+1)'), '-1+sqrt(2)')

    def test_evalc(self):
        self.assertEqual(self.s('evalc', 'exp(I*x)'), 'I*sin(x)+cos(x)')
        self.assertEqual(self.s('evalc', '(1+I)*(1-I)'), '2')

    def test_evalb(self):
        self.assertEqual(self.s('evalb', '2=2'), 'true')
        self.assertEqual(self.s('evalb', '1<2'), 'true')
        self.assertEqual(self.s('evalb', 'x=y'), 'false')
        self.assertEqual(self.s('evalb', 'sqrt(2)^2=2'), 'true')
        self.assertIn('cannot determine', self.err('evalb', 'x<2'))

    def test_is(self):
        self.assertEqual(self.s('is', 'x>0', assume=['0<x']), 'true')
        self.assertEqual(self.s('is', 'x>0'), 'FAIL')
        self.assertEqual(self.s('is', 'x', 'positive', assume=['0<x']), 'true')

    def test_testeq(self):
        self.assertEqual(self.s('testeq', 'sin(x)^2+cos(x)^2=1'), 'true')
        self.assertEqual(self.s('testeq', '(x+1)^2=x^2+1'), 'false')

    def test_convert_parfrac(self):
        self.eqv('convert', '1/(x^2-1)', 'parfrac', 'x', expect='1/(2*(x-1))-1/(2*(x+1))')
        r = self.ok('convert', '1/(x^2-1)', 'parfrac', 'x')
        self.assertEqual(len(r['add']), 2)
        self.assertEqual(self.s('convert', '(x+1)/(x^2-1)', 'parfrac', 'x'), '1/(x-1)')

    def test_convert_targets(self):
        self.assertEqual(self.s('convert', 'cos(x)', 'exp'), 'exp(I*x)/2+exp(-I*x)/2')
        self.assertEqual(self.s('convert', 'tan(x)', 'sincos'), 'sin(x)/cos(x)')
        self.assertEqual(self.s('convert', 'sec(x)', 'sincos'), '1/cos(x)')
        self.assertEqual(self.s('convert', '0.25', 'rational'), '1/4')
        self.assertEqual(self.s('convert', '1/3', 'float'), '0.3333333333')
        self.assertEqual(self.s('convert', '{3,1,2}', 'list'), '[1,2,3]')
        self.assertEqual(self.s('convert', '[3,1,3]', 'set'), '{1,3}')
        self.assertEqual(self.s('convert', '10', 'base', '2'), '[0,1,0,1]')
        self.assertEqual(self.s('convert', 'x^3+2*x^2+x', 'horner', 'x'), 'x*(x*(x+2)+1)')

    def test_convert_polynom_drops_order_term(self):
        ser = self.ok('series', 'sin(x)', 'x=0', '6')
        r = engine().send({'op': 'call', 'name': 'convert', 'args': [ser, X('polynom')], 'opts': {}})
        self.assertEqual(S(r['result']), 'x^5/120-x^3/6+x')

    def test_subs_and_eval(self):
        self.assertEqual(self.s('subs', 'x=2', 'x^2+x'), '6')
        self.assertEqual(self.s('subs', 'x=y,y=1', 'x+y'), '2')
        self.assertEqual(self.s('eval', 'x^2+y', '[x=1,y=2]'), '3')
        self.assertEqual(self.s('eval', 'x+y', 'x=3'), 'y+3')

    def test_constants_and_exp(self):
        self.assertEqual(self.s('exp', '1'), 'exp(1)')
        self.assertEqual(self.s('ln', 'exp(2)'), '2')
        self.assertEqual(self.s('simplify', 'exp(1)^2'), 'exp(2)')
        self.assertEqual(self.s('simplify', 'I^2'), '-1')

    def test_E_is_just_a_name(self):
        self.assertEqual(self.s('diff', 'E^x', 'x'), 'E^x*ln(E)')

    def test_division_by_zero_error(self):
        r = engine().send({'op': 'call', 'name': 'expand', 'args': [X('1/0')], 'opts': {}})
        self.assertFalse(r['ok'])
        self.assertIn('division by zero', r['error'])

    def test_infinity_and_pi_constants(self):
        self.assertEqual(self.s('simplify', 'infinity+1'), 'infinity')
        self.assertEqual(self.s('simplify', 'Pi*I*I'), '-Pi')

    def test_rational_nodes_and_big_integers(self):
        r = engine().send({'op': 'call', 'name': 'expand', 'args': [{'rat': ['2', '4']}], 'opts': {}})
        self.assertEqual(r['result'], {'rat': ['1', '2']})
        big = '1' + '0' * 40
        r = engine().send({'op': 'call', 'name': 'expand', 'args': [{'add': [{'int': big}, {'int': '1'}]}], 'opts': {}})
        self.assertEqual(r['result'], {'int': '1' + '0' * 39 + '1'})

    def test_output_shapes_of_basic_nodes(self):
        r = self.ok('expand', '(x-y)*2')
        self.assertEqual(r, {'add': [{'mul': [{'int': '2'}, {'sym': 'x'}]},
                                     {'mul': [{'int': '-2'}, {'sym': 'y'}]}]})
        r = self.ok('expand', '-y')
        self.assertEqual(r, {'mul': [{'int': '-1'}, {'sym': 'y'}]})
        r = self.ok('expand', 'sqrt(x)')
        self.assertEqual(r, {'pow': [{'sym': 'x'}, {'rat': ['1', '2']}]})
        r = self.ok('expand', 'x/y')
        self.assertEqual(r, {'mul': [{'sym': 'x'}, {'pow': [{'sym': 'y'}, {'int': '-1'}]}]})


class TestAssumptions(Base):
    def test_positive_real_assumption(self):
        self.assertEqual(self.s('simplify', 'sqrt(x^2)', assume=['0<x']), 'x')
        self.assertEqual(self.s('simplify', 'abs(x)', assume=['0<x']), 'x')
        self.assertEqual(self.s('simplify', 'abs(x)', assume=['x<0']), '-x')

    def test_lower_bound_assumption(self):
        self.assertEqual(self.s('simplify', 'sqrt(x^2)', assume=['1<x']), 'x')

    def test_negative_and_nonpositive(self):
        self.assertEqual(self.s('simplify', 'abs(x)', assume=['x<=0']), '-x')

    def test_type_assumptions(self):
        self.assertEqual(self.s('simplify', 'sin(n*Pi)', assume=[TYPE('n', 'integer')]), '0')
        self.assertEqual(self.s('simplify', 'sin(n*Pi)'), 'sin(Pi*n)')
        self.assertEqual(self.s('simplify', 'ln(exp(x))', assume=[TYPE('x', 'real')]), 'x')
        self.assertEqual(self.s('simplify', 'ln(exp(x))'), 'ln(exp(x))')

    def test_assumption_filters_solutions(self):
        self.assertEqual(self.s('solve', 'x^2-4=0', 'x', assume=['0<x']), '2')
        self.assertEqual(self.s('solve', 'x^2-4=0', 'x', assume=['x<0']), '-2')

    def test_results_use_plain_names(self):
        r = self.ok('simplify', 'sqrt(x^2)', assume=['0<x'])
        self.assertEqual(r, {'sym': 'x'})

    def test_combined_assumptions_with_and(self):
        a = {'and': [X('0<x'), X('0<y')]}
        self.assertEqual(self.s('combine', 'ln(x)+ln(y)', 'ln', assume=[a]), 'ln(x*y)')

    def test_inconsistent_assumptions_do_not_crash(self):
        self.assertEqual(self.s('expand', 'x+x', assume=['0<x', 'x<0']), '2*x')

    def test_is_uses_assumptions(self):
        self.assertEqual(self.s('is', 'x^2>=0', assume=[TYPE('x', 'real')]), 'true')


class TestCalculus(Base):
    def test_int_no_constant(self):
        self.assertEqual(self.s('int', 'x^2', 'x'), 'x^3/3')
        self.assertEqual(self.s('int', 'sin(x)', 'x'), '-cos(x)')
        self.assertEqual(self.s('int', '1/x', 'x'), 'ln(x)')

    def test_int_definite(self):
        self.assertEqual(self.s('int', 'x^2', 'x=0..1'), '1/3')
        self.assertEqual(self.s('int', 'exp(-x^2)', 'x=-infinity..infinity'), 'sqrt(Pi)')
        self.assertEqual(self.s('int', '1/(x^2+1)', 'x=0..1'), 'Pi/4')
        self.assertEqual(self.s('int', 'x*exp(-x)', 'x=0..infinity'), '1')

    def test_int_divergent_and_singular(self):
        self.assertEqual(self.s('int', '1/x^2', 'x=-1..1'), 'infinity')
        self.assertEqual(self.s('int', 'ln(x)', 'x=0..1'), '-1')

    def test_int_symbolic_exponent_is_generic(self):
        self.assertEqual(self.s('int', 'x^n', 'x'), 'x^(n+1)/(n+1)')

    def test_int_parameters_in_bounds(self):
        self.assertEqual(self.s('int', 'x^2', 'x=0..a'), 'a^3/3')

    def test_int_unevaluated_when_unknown(self):
        res = self.ok('int', 'f(x)', 'x')
        self.assertEqual(S(res), 'int(f(x),x)')

    def test_inert_and_value(self):
        self.assertEqual(self.s('Int', 'x^2', 'x=0..1'), 'Int(x^2,x=0..1)')
        node = engine().send({'op': 'call', 'name': 'Int', 'args': [X('x^2'), X('x=0..1')], 'opts': {}})['result']
        r = engine().send({'op': 'call', 'name': 'value', 'args': [node], 'opts': {}})
        self.assertEqual(S(r['result']), '1/3')

    def test_diff(self):
        self.assertEqual(self.s('diff', 'sin(x)*x', 'x'), 'x*cos(x)+sin(x)')
        self.assertEqual(self.s('diff', 'x^4', 'x', 'x'), '12*x^2')
        self.assertEqual(self.s('diff', 'x^2*y', 'x', 'y'), '2*x')
        self.assertEqual(self.s('diff', 'ln(x)', 'x'), '1/x')

    def test_diff_of_unknown_function_stays_symbolic(self):
        self.assertEqual(self.s('diff', 'y(x)', 'x'), 'diff(y(x),x)')
        self.assertEqual(self.s('diff', 'y(x)^2', 'x'), '2*y(x)*diff(y(x),x)')

    def test_diff_needs_a_variable(self):
        self.assertIn('wrong number', self.err('diff', 'x^2'))

    def test_diff_invalid_variable(self):
        self.assertIn('invalid differentiation variable', self.err('diff', 'x^2', '2'))

    def test_D_operator(self):
        self.assertEqual(self.s('D', 'sin'), 'cos')
        self.assertEqual(self.s('D', 'exp'), 'exp')
        proc = {'proc': {'params': ['x'], 'body': X('x^2*sin(x)')}}
        r = engine().send({'op': 'call', 'name': 'D', 'args': [proc], 'opts': {}})
        self.assertEqual(S(r['result']), '(x)->x^2*cos(x)+2*x*sin(x)')

    def test_D_partial_derivative_index(self):
        proc = {'proc': {'params': ['x', 'y'], 'body': X('x^2*y')}}
        r = engine().send({'op': 'call', 'name': 'D', 'args': [proc], 'idx': [X('2')], 'opts': {}})
        self.assertEqual(S(r['result']), '(x,y)->x^2')

    def test_D_composed_with_itself(self):
        proc = {'proc': {'params': ['x'], 'body': X('x^4')}}
        r = engine().send({'op': 'call', 'name': 'D@@2', 'args': [proc], 'opts': {}})
        self.assertEqual(S(r['result']), '(x)->12*x^2')

    def test_D_unknown_stays_unevaluated(self):
        self.assertEqual(self.s('D', 'f'), 'D(f)')

    def test_limit(self):
        self.assertEqual(self.s('limit', 'sin(x)/x', 'x=0'), '1')
        self.assertEqual(self.s('limit', '(1+1/n)^n', 'n=infinity'), 'exp(1)')
        self.assertEqual(self.s('limit', '(sin(x)-x)/x^3', 'x=0'), '-1/6')

    def test_limit_directions(self):
        self.assertEqual(self.s('limit', '1/x', 'x=0'), 'undefined')
        self.assertEqual(self.s('limit', '1/x', 'x=0', 'right'), 'infinity')
        self.assertEqual(self.s('limit', '1/x', 'x=0', 'left'), '-infinity')
        self.assertEqual(self.s('limit', 'abs(x)/x', 'x=0'), 'undefined')
        self.assertEqual(self.s('limit', '1/x^2', 'x=0'), 'infinity')

    def test_limit_bounded_oscillation(self):
        self.assertEqual(self.s('limit', 'sin(1/x)', 'x=0'), '-1..1')

    def test_series_has_order_term(self):
        res = self.ok('series', 'sin(x)', 'x=0', '6')
        self.assertEqual(S(res), 'x-x^3/6+x^5/120+O(x^6)')
        self.assertEqual(res['add'][-1], {'fn': 'O', 'args': [{'pow': [{'sym': 'x'}, {'int': '6'}]}]})

    def test_series_variants(self):
        self.assertEqual(self.s('series', '1/(1-x)', 'x', '4'), '1+x+x^2+x^3+O(x^4)')
        self.assertEqual(self.s('series', 'exp(x)', 'x=1', '3'), 'exp(1)+exp(1)*(x-1)+exp(1)*(x-1)^2/2+O((x-1)^3)')
        self.assertEqual(self.s('series', 'cos(x)', 'x'), '1-x^2/2+x^4/24+O(x^6)')

    def test_taylor_and_mtaylor(self):
        self.assertEqual(self.s('taylor', 'exp(x)', 'x=0', '4'), '1+x+x^2/2+x^3/6+O(x^4)')
        self.assertIn('taylor', self.err('taylor', '1/x', 'x=0', '4'))
        self.assertEqual(self.s('mtaylor', 'exp(x+y)', '[x,y]', '3'), 'x^2/2+x*y+x+y^2/2+y+1')

    def test_sum_and_product(self):
        self.assertEqual(self.s('sum', 'k^2', 'k=1..n'), 'n^3/3+n^2/2+n/6')
        self.assertEqual(self.s('sum', '1/k^2', 'k=1..infinity'), 'Pi^2/6')
        self.assertEqual(self.s('sum', '1/(k*(k+1))', 'k=1..infinity'), '1')
        self.assertEqual(self.s('sum', 'k', 'k'), 'k^2/2-k/2')
        self.assertEqual(self.s('product', 'k', 'k=1..5'), '120')
        self.assertEqual(self.s('sum', 'x^k', 'k=0..infinity'), '1/(1-x)')

    def test_inert_sum_value(self):
        node = engine().send({'op': 'call', 'name': 'Sum', 'args': [X('k'), X('k=1..4')], 'opts': {}})['result']
        self.assertEqual(S(node), 'Sum(k,k=1..4)')
        r = engine().send({'op': 'call', 'name': 'value', 'args': [node], 'opts': {}})
        self.assertEqual(S(r['result']), '10')

    def test_residue_singular(self):
        self.assertEqual(self.s('residue', '1/(x*(x-1))', 'x=0'), '-1')
        self.assertEqual(self.s('singular', '1/(x*(x-1))', 'x'), '{x=0,x=1}')

    def test_implicitdiff(self):
        self.assertEqual(self.s('implicitdiff', 'x^2+y^2=1', 'y', 'x'), '-x/y')

    def test_extrema(self):
        self.assertEqual(self.s('maximize', '-x^2+1', 'x'), '1')
        self.assertEqual(self.s('minimize', 'x^2-2*x', 'x=0..3'), '-1')
        self.assertEqual(self.s('extrema', 'x^2+y^2', '{x+y=1}', '{x,y}'), '{1/2}')


class TestSolve(Base):
    def test_one_variable_returns_a_sequence(self):
        res = self.ok('solve', 'x^2-4=0', 'x')
        self.assertEqual(res, {'seq': [{'int': '2'}, {'int': '-2'}]})

    def test_expression_without_equals(self):
        self.assertEqual(self.s('solve', 'x^2-4', 'x'), '2,-2')

    def test_complex_roots(self):
        self.assertEqual(self.s('solve', 'x^2+1=0', 'x'), 'I,-I')

    def test_irrational_roots(self):
        self.assertEqual(self.s('solve', 'x^2-2=0', 'x'), 'sqrt(2),-sqrt(2)')

    def test_no_solution_is_null(self):
        self.assertEqual(self.ok('solve', 'x+1=x', 'x'), {'seq': []})

    def test_identity_returns_the_variable(self):
        self.assertEqual(self.s('solve', 'x=x', 'x'), 'x')

    def test_single_solution_is_not_wrapped(self):
        self.assertEqual(self.ok('solve', 'exp(x)=2', 'x'), self.ok('ln', '2'))

    def test_linear_system_is_a_set_of_equations(self):
        res = self.ok('solve', '{x+y=3,x-y=1}', '{x,y}')
        self.assertEqual(S(res), '{x=2,y=1}')
        self.assertIn('set', res)

    def test_system_with_list_of_unknowns(self):
        self.assertEqual(self.s('solve', '{x+y=3,x-y=1}', '[x,y]'), '[[x=2,y=1]]')

    def test_several_solutions_are_a_sequence_of_sets(self):
        res = self.ok('solve', '{x^2+y^2=1,x=y}', '{x,y}')
        self.assertEqual(len(res['seq']), 2)
        self.assertTrue(all('set' in s for s in res['seq']))

    def test_underdetermined_system(self):
        self.assertEqual(self.s('solve', '{x+y=3}', '{x,y}'), '{x=3-y,y=y}')

    def test_set_of_unknowns_for_one_equation(self):
        self.assertEqual(self.s('solve', 'x^2-4=0', '{x}'), '{x=2},{x=-2}')

    def test_parametric_quadratic(self):
        res = self.ok('solve', 'a*x^2+b*x+c=0', 'x')
        self.assertEqual(len(res['seq']), 2)

    def test_rootof_for_quintic(self):
        res = self.ok('solve', 'x^5-x+1=0', 'x')
        self.assertEqual(len(res['seq']), 5)
        self.assertEqual(res['seq'][0]['fn'], 'RootOf')

    def test_trig_equation(self):
        sols = self.ok('solve', 'sin(x)=1/2', 'x')
        self.assertIn(S(sols), ('Pi/6,5*Pi/6', '5*Pi/6,Pi/6'))

    def test_absolute_value_equation(self):
        self.assertEqual(self.s('solve', 'abs(x-1)=2', 'x'), '3,-1')

    def test_radical_equation_rejects_extraneous_root(self):
        self.assertEqual(self.s('solve', 'sqrt(x+1)=x-1', 'x'), '3')

    def test_unsolvable_returns_input(self):
        self.assertEqual(self.s('solve', 'sin(x)=x', 'x'), 'solve(sin(x)=x,x)')

    def test_invalid_unknown_is_an_error(self):
        self.assertIn('wrong number', self.err('solve', 'x^2=1', '3'))

    def test_solve_without_variable(self):
        self.assertEqual(self.s('solve', 'x^2-2=0'), 'sqrt(2),-sqrt(2)')

    def test_inequality(self):
        self.assertEqual(self.s('solve', 'x^2>4', 'x'), 'RealRange(-infinity,Open(-2)),RealRange(Open(2),infinity)')
        self.assertEqual(self.s('solve', 'x^2>4', '{x}'), '{x<-2},{2<x}')
        self.assertEqual(self.s('solve', 'x^2<=4', 'x'), 'RealRange(-2,2)')

    def test_fsolve_single_root(self):
        self.assertEqual(self.s('fsolve', 'cos(x)=x', 'x'), '0.7390851332')
        self.assertEqual(self.s('fsolve', 'cos(x)=x', 'x=0..1'), '0.7390851332')

    def test_fsolve_polynomial_all_real_roots(self):
        self.assertEqual(self.s('fsolve', 'x^3-x=0', 'x'), '-1.,0.,1.')

    def test_fsolve_digits(self):
        self.assertEqual(self.s('fsolve', 'x^3-2*x-5=0', 'x', digits=15), '2.09455148154233')

    def test_fsolve_system(self):
        self.assertEqual(self.s('fsolve', '{x^2+y^2=1,x=y}', '{x,y}'), '{x=0.7071067812,y=0.7071067812}')

    def test_isolve(self):
        self.assertEqual(self.s('isolve', 'x^2=4'), '{x=2},{x=-2}')
        self.assertEqual(self.s('isolve', '3*x+7*y=5'), '{x=7*_Z1-10,y=5-3*_Z1}')

    def test_rsolve(self):
        self.assertEqual(self.s('rsolve', '{f(n+1)=f(n)+2,f(0)=1}', 'f(n)'), '2*n+1')
        self.assertEqual(self.s('rsolve', 'f(n)=2*f(n-1)', 'f(n)'), '2^n*f(0)')


class TestDsolve(Base):
    def test_first_order_uses_maple_constants(self):
        res = self.ok('dsolve', 'diff(y(x),x)=y(x)', 'y(x)')
        self.assertEqual(S(res), 'y(x)=_C1*exp(x)')
        self.assertIn('eq', res)

    def test_second_order(self):
        self.assertEqual(self.s('dsolve', 'diff(y(x),x,x)+y(x)=0', 'y(x)'), 'y(x)=_C1*sin(x)+_C2*cos(x)')

    def test_initial_condition(self):
        self.assertEqual(self.s('dsolve', '{diff(y(x),x)=y(x),y(0)=1}', 'y(x)'), 'y(x)=exp(x)')

    def test_derivative_initial_condition_as_D_application(self):
        dy0 = {'fn': {'fn': 'D', 'args': [{'sym': 'y'}]}, 'args': [{'int': '0'}]}
        ode = {'set': [X('diff(y(x),x,x)+y(x)=0'), X('y(0)=1'), {'eq': [dy0, X('0')]}]}
        r = engine().send({'op': 'call', 'name': 'dsolve', 'args': [ode, X('y(x)')], 'opts': {}})
        self.assertEqual(S(r['result']), 'y(x)=cos(x)')

    def test_derivative_initial_condition_as_apply_node(self):
        dy0 = {'apply': [{'fn': 'D', 'args': [{'sym': 'y'}]}, [{'int': '0'}]]}
        ode = {'set': [X('diff(y(x),x,x)+y(x)=0'), X('y(0)=1'), {'eq': [dy0, X('0')]}]}
        r = engine().send({'op': 'call', 'name': 'dsolve', 'args': [ode, X('y(x)')], 'opts': {}})
        self.assertEqual(S(r['result']), 'y(x)=cos(x)')

    def test_derivative_initial_condition_as_fnidx(self):
        dy0 = {'fn': {'fnidx': ['D', [{'int': '1'}], [{'sym': 'y'}]]}, 'args': [{'int': '0'}]}
        ode = {'set': [X('diff(y(x),x,x)+y(x)=0'), X('y(0)=1'), {'eq': [dy0, X('0')]}]}
        r = engine().send({'op': 'call', 'name': 'dsolve', 'args': [ode, X('y(x)')], 'opts': {}})
        self.assertEqual(S(r['result']), 'y(x)=cos(x)')

    def test_linear_with_forcing(self):
        self.assertEqual(self.s('dsolve', 'diff(y(x),x)+2*y(x)=exp(-x)', 'y(x)'), 'y(x)=(_C1*exp(-x)+1)*exp(-x)')

    def test_separable(self):
        self.assertEqual(self.s('dsolve', 'diff(y(x),x)=y(x)^2', 'y(x)'), 'y(x)=-1/(_C1+x)')

    def test_system(self):
        sys_ = {'set': [X('diff(x(t),t)=y(t)'), X('diff(y(t),t)=-x(t)'), X('x(0)=1'), X('y(0)=0')]}
        funcs = {'set': [X('x(t)'), X('y(t)')]}
        r = engine().send({'op': 'call', 'name': 'dsolve', 'args': [sys_, funcs], 'opts': {}})
        self.assertEqual(S(r['result']), '{x(t)=cos(t),y(t)=-sin(t)}')

    def test_numeric_is_a_clear_error(self):
        self.assertIn('not supported yet', self.err('dsolve', 'diff(y(x),x)=y(x)', 'y(x)', 'numeric'))

    def test_odetest(self):
        self.assertEqual(self.s('odetest', 'y(x)=_C1*exp(x)', 'diff(y(x),x)=y(x)'), '0')
        self.assertEqual(self.s('odetest', 'y(x)=exp(2*x)', 'diff(y(x),x)=y(x)'), 'exp(2*x)')

    def test_odetest_checks_initial_conditions_too(self):
        res = self.ok('odetest', 'y(x)=cos(x)', '{diff(y(x),x,x)+y(x)=0,y(0)=1}')
        self.assertEqual(S(res), '0,0')
        res = self.ok('odetest', 'y(x)=cos(x)', '{diff(y(x),x,x)+y(x)=0,y(0)=2}')
        self.assertNotEqual(S(res), '0,0')

    def test_recurrence_with_two_initial_values(self):
        self.eqv('rsolve', '{f(n+2)=f(n+1)+f(n),f(0)=0,f(1)=1}', 'f(n)',
                 expect='(((1+sqrt(5))/2)^n-((1-sqrt(5))/2)^n)/sqrt(5)')

    def test_pdsolve(self):
        res = self.s('pdsolve', 'diff(u(x,y),x)+diff(u(x,y),y)=0', 'u(x,y)')
        self.assertEqual(res, 'u(x,y)=_F1(x-y)')


class TestLinearAlgebra(Base):
    def setUp(self):
        self.A = MAT([['1', '2'], ['3', '4']])
        self.D = MAT([['2', '0'], ['0', '3']])

    def expandNode(self, node):
        r = engine().send({'op': 'call', 'name': 'expand', 'args': [node], 'opts': {}})
        return r

    def test_constructors(self):
        self.assertEqual(self.s('Matrix', '[[1,2],[3,4]]'), 'Matrix[[1,2],[3,4]]')
        self.assertEqual(self.s('Matrix', '2', '2'), 'Matrix[[0,0],[0,0]]')
        self.assertEqual(self.s('Matrix', '2', '3', '[1,2,3,4,5,6]'), 'Matrix[[1,2,3],[4,5,6]]')
        self.assertEqual(self.s('Vector', '[1,2,3]'), 'Vector[1,2,3]')
        r = engine().send({'op': 'call', 'name': 'Vector', 'args': [X('[1,2]')], 'idx': [X('row')], 'opts': {}})
        self.assertEqual(r['result'], {'vector': [{'int': '1'}, {'int': '2'}], 'orient': 'row'})
        self.assertEqual(self.s('IdentityMatrix', '2'), 'Matrix[[1,0],[0,1]]')
        self.assertEqual(self.s('ZeroMatrix', '1', '2'), 'Matrix[[0,0]]')
        self.assertEqual(self.s('DiagonalMatrix', '[1,2]'), 'Matrix[[1,0],[0,2]]')

    def test_determinant_inverse_transpose(self):
        self.assertEqual(self.s('Determinant', self.A), '-2')
        self.assertEqual(self.s('MatrixInverse', self.A), 'Matrix[[-2,1],[3/2,-1/2]]')
        self.assertEqual(self.s('Transpose', self.A), 'Matrix[[1,3],[2,4]]')
        self.assertIn('singular', self.err('MatrixInverse', MAT([['1', '2'], ['2', '4']])))

    def test_trace_rank(self):
        self.assertEqual(self.s('Trace', self.A), '5')
        self.assertEqual(self.s('Rank', MAT([['1', '2'], ['2', '4']])), '1')

    def test_eigenvalues_is_a_column_vector(self):
        r = self.ok('Eigenvalues', self.D)
        self.assertEqual(r['orient'], 'column')
        self.assertEqual(S(r), 'Vector[2,3]')
        self.assertEqual(self.s('Eigenvalues', self.A), 'Vector[5/2-sqrt(33)/2,5/2+sqrt(33)/2]')

    def test_eigenvalues_of_float_matrix_are_numeric(self):
        r = self.ok('Eigenvalues', MAT([['1.', '0.5'], ['0.5', '2.']]))
        vals = [float(x['float']) for x in r['vector']]
        self.assertAlmostEqual(vals[0], 1.5 - 0.5 ** 0.5, places=12)
        self.assertAlmostEqual(vals[1], 1.5 + 0.5 ** 0.5, places=12)
        r = self.ok('Eigenvalues', MAT([['1.', '2.'], ['-3.', '1.']]))
        self.assertTrue(all('I' in S(x) for x in r['vector']), S(r))
        r = self.ok('Eigenvectors', MAT([['2.', '0.'], ['0.', '3.']]))
        self.assertEqual(len(r['seq']), 2)

    def test_eigenvectors_returns_vector_and_matrix(self):
        r = self.ok('Eigenvectors', self.D)
        self.assertEqual(S(r), 'Vector[2,3],Matrix[[1,0],[0,1]]')
        self.assertIn('vector', r['seq'][0])
        self.assertIn('matrix', r['seq'][1])

    def test_eigenvectors_defective(self):
        r = self.ok('Eigenvectors', MAT([['2', '1'], ['0', '2']]))
        self.assertEqual(S(r), 'Vector[2,2],Matrix[[1,0],[0,0]]')

    def test_spaces(self):
        sing = MAT([['1', '2'], ['2', '4']])
        self.assertEqual(self.s('NullSpace', sing), '{Vector[-2,1]}')
        self.assertEqual(self.s('ColumnSpace', sing), '[Vector[1,2]]')
        self.assertEqual(self.s('RowSpace', sing), '[RowVector[1,2]]')

    def test_linear_solve(self):
        self.assertEqual(self.s('LinearSolve', self.A, VEC(['5', '6'])), 'Vector[-4,9/2]')

    def test_row_reduction(self):
        self.assertEqual(self.s('ReducedRowEchelonForm', MAT([['1', '2', '3'], ['4', '5', '6']])),
                         'Matrix[[1,0,-1],[0,1,2]]')
        self.assertEqual(self.s('GaussianElimination', MAT([['1', '2'], ['3', '4']])), 'Matrix[[1,2],[0,-2]]')

    def test_decompositions(self):
        r = self.ok('LUDecomposition', self.A)
        self.assertEqual(len(r['seq']), 3)
        self.assertEqual(S(r['seq'][1]), 'Matrix[[1,0],[3,1]]')
        self.assertEqual(S(r['seq'][2]), 'Matrix[[1,2],[0,-2]]')
        q = self.ok('QRDecomposition', self.A)
        self.assertEqual(len(q['seq']), 2)

    def test_polynomials_of_a_matrix(self):
        self.assertEqual(self.s('CharacteristicPolynomial', self.A, 'x'), 'x^2-5*x-2')
        self.assertEqual(self.s('MinimalPolynomial', self.D, 'x'), 'x^2-5*x+6')
        self.assertEqual(self.s('JordanForm', MAT([['2', '1'], ['0', '2']])), 'Matrix[[2,1],[0,2]]')

    def test_vector_products_and_norms(self):
        self.assertEqual(self.s('DotProduct', VEC(['1', '2', '3']), VEC(['4', '5', '6'])), '32')
        self.assertEqual(self.s('CrossProduct', VEC(['1', '0', '0']), VEC(['0', '1', '0'])), 'Vector[0,0,1]')
        self.assertEqual(self.s('Norm', VEC(['3', '4']), '2'), '5')
        self.assertEqual(self.s('Norm', VEC(['3', '4'])), '4')
        self.assertEqual(self.s('Normalize', VEC(['3', '4'])), 'Vector[3/5,4/5]')

    def test_matrix_functions(self):
        self.assertEqual(self.s('MatrixExponential', MAT([['0', '1'], ['0', '0']]), 't'), 'Matrix[[1,t],[0,1]]')
        self.assertEqual(self.s('MatrixPower', self.A, '2'), 'Matrix[[7,10],[15,22]]')
        self.assertEqual(self.s('IsDefinite', self.D), 'true')
        self.assertEqual(self.s('IsDefinite', MAT([['1', '2'], ['2', '1']])), 'false')
        self.assertEqual(self.s('Dimension', self.A), '2,2')

    def test_eigenvalues_of_irreducible_cubic_use_rootof(self):
        m = MAT([['1', '2', '3'], ['4', '5', '6'], ['7', '8', '10']])
        r = self.ok('Eigenvalues', m)
        self.assertTrue(all(v['fn'] == 'RootOf' for v in r['vector']))
        self.assertEqual(S(r['vector'][0]), 'RootOf(_Z^3-16*_Z^2-12*_Z+3,index=1)')

    def test_float_matrices(self):
        f = MAT([['1.0', '2.0'], ['3.0', '4.0']])
        self.assertEqual(self.s('Determinant', f), '-2.')
        # hardware-float precision, as Maple's LAPACK path
        self.assertEqual(self.s('Eigenvalues', f), 'Vector[-0.372281323269014,5.37228132326901]')

    def test_linear_solve_underdetermined_and_inconsistent(self):
        sing = MAT([['1', '1'], ['2', '2']])
        self.assertEqual(self.s('LinearSolve', sing, VEC(['1', '2'])), 'Vector[1-_t1,_t1]')
        self.assertIn('inconsistent', self.err('LinearSolve', sing, VEC(['1', '3'])))

    def test_complex_eigenvalues(self):
        self.assertEqual(self.s('Eigenvalues', MAT([['0', '1'], ['-1', '0']])), 'Vector[-I,I]')

    def test_gram_schmidt_and_singular_values(self):
        vs = {'list': [VEC(['1', '1']), VEC(['1', '0'])]}
        self.assertEqual(self.s('GramSchmidt', vs), '[Vector[1,1],Vector[1/2,-1/2]]')
        self.assertEqual(self.s('SingularValues', self.D), 'Vector[3,2]')

    def test_matrix_arithmetic_through_nodes(self):
        add = {'add': [self.A, self.D]}
        self.assertEqual(S(self.expandNode(add)['result']), 'Matrix[[3,2],[3,7]]')
        mul = {'mul': [X('2'), self.A]}
        self.assertEqual(S(self.expandNode(mul)['result']), 'Matrix[[2,4],[6,8]]')
        inv = {'pow': [self.A, X('-1')]}
        self.assertEqual(S(self.expandNode(inv)['result']), 'Matrix[[-2,1],[3/2,-1/2]]')

    def test_matrix_product_with_dot_operator(self):
        prod = {'fn': '.', 'args': [self.A, VEC(['1', '1'])]}
        self.assertEqual(S(self.expandNode(prod)['result']), 'Vector[3,7]')

    def test_symbolic_matrices(self):
        m = MAT([['a', 'b'], ['c', 'd']])
        self.assertEqual(self.s('Determinant', m), 'a*d-b*c')

    def test_dimension_mismatch_error(self):
        bad = {'add': [self.A, MAT([['1', '2', '3']])]}
        self.assertFalse(self.expandNode(bad)['ok'])


class TestPolynomials(Base):
    def test_coefficients(self):
        self.assertEqual(self.s('coeff', '3*x^2+2*x+1', 'x', '2'), '3')
        self.assertEqual(self.s('coeff', '3*x^2+2*x+1', 'x'), '2')
        self.assertEqual(self.s('coeff', '3*x^2+2*x+1', 'x^2'), '3')
        self.assertEqual(self.s('coeffs', '3*x^2+2*x+1', 'x'), '1,2,3')
        self.assertEqual(self.s('lcoeff', '3*x^2+2*x+1', 'x'), '3')
        self.assertEqual(self.s('tcoeff', '3*x^2+2*x+5', 'x'), '5')

    def test_degrees(self):
        self.assertEqual(self.s('degree', 'x^3+x', 'x'), '3')
        self.assertEqual(self.s('ldegree', 'x^3+x', 'x'), '1')
        self.assertEqual(self.s('degree', 'x^2*y^3+1'), '5')

    def test_numer_denom(self):
        self.assertEqual(self.s('numer', '3/4'), '3')
        self.assertEqual(self.s('denom', 'x/(x+1)+1/x'), 'x*(x+1)')

    def test_division(self):
        self.assertEqual(self.s('quo', 'x^3+1', 'x+1', 'x'), 'x^2-x+1')
        self.assertEqual(self.s('rem', 'x^3+2', 'x+1', 'x'), '1')

    def test_gcd_lcm(self):
        self.assertEqual(self.s('gcd', 'x^2-1', 'x^2+2*x+1'), 'x+1')
        self.assertEqual(self.s('gcd', '12', '18'), '6')
        self.assertEqual(self.s('lcm', '4', '6'), '12')

    def test_resultant_discriminant(self):
        self.assertEqual(self.s('resultant', 'x^2-1', 'x-2', 'x'), '3')
        self.eqv('discrim', 'a*x^2+b*x+c', 'x', expect='b^2-4*a*c')

    def test_content_primpart_sqrfree(self):
        self.assertEqual(self.s('content', '6*x^2+9*x+12', 'x'), '3')
        self.assertEqual(self.s('primpart', '6*x^2+9*x+12', 'x'), '2*x^2+3*x+4')
        self.assertEqual(self.s('sqrfree', '(x-1)^2*(x+2)'), '[1,[[x+2,1],[x-1,2]]]')

    def test_roots_over_the_rationals(self):
        self.assertEqual(self.s('roots', 'x^2-4', 'x'), '[[2,1],[-2,1]]')
        self.assertEqual(self.s('roots', 'x^2-2', 'x'), '[]')
        self.assertEqual(self.s('roots', 'x^2+1', 'x', 'I'), '[[I,1],[-I,1]]')

    def test_crootof_alias(self):
        self.assertEqual(self.s('CRootOf', '_Z^2-2'), 'RootOf(_Z^2-2)')

    def test_rootof_and_allvalues(self):
        self.assertEqual(self.s('RootOf', '_Z^2-2'), 'RootOf(_Z^2-2)')
        self.assertEqual(self.s('allvalues', 'RootOf(_Z^2-2)'), 'sqrt(2),-sqrt(2)')
        r = self.ok('RootOf', '_Z^5-_Z+1', 'index=1')
        self.assertEqual(r['fn'], 'RootOf')
        self.assertEqual(S(r['args'][1]), 'index=1')

    def test_groebner_basis_and_interp(self):
        self.assertEqual(self.s('Groebner[Basis]', '[x^2+y,x*y-1]', 'plex(x,y)'), '[x+y^2,y^3+1]')
        self.assertEqual(self.s('interp', '[1,2,3]', '[1,4,9]', 'v'), 'v^2')

    def test_polynomial_over_many_variables(self):
        self.assertEqual(self.s('coeff', 'a*x^2+b*x+c', 'x', '1'), 'b')


class TestNumberTheory(Base):
    def test_primes(self):
        self.assertEqual(self.s('isprime', '97'), 'true')
        self.assertEqual(self.s('isprime', '91'), 'false')
        self.assertEqual(self.s('ithprime', '10'), '29')
        self.assertEqual(self.s('nextprime', '10'), '11')
        self.assertEqual(self.s('prevprime', '10'), '7')

    def test_factorisation(self):
        self.assertEqual(self.s('ifactor', '12'), '2^2*3')
        self.assertEqual(self.s('ifactors', '12'), '[1,[[2,2],[3,1]]]')
        self.assertEqual(self.s('ifactors', '-12'), '[-1,[[2,2],[3,1]]]')

    def test_integer_division(self):
        self.assertEqual(self.s('iquo', '17', '5'), '3')
        self.assertEqual(self.s('irem', '17', '5'), '2')
        self.assertEqual(self.s('iquo', '-7', '2'), '-3')
        self.assertEqual(self.s('irem', '-7', '2'), '-1')
        self.assertEqual(self.s('igcd', '12', '18'), '6')
        self.assertEqual(self.s('ilcm', '4', '6'), '12')

    def test_modular_arithmetic(self):
        self.assertEqual(self.s('mod', '17', '5'), '2')
        self.assertEqual(self.s('mod', '-1', '5'), '4')
        self.assertEqual(self.s('modp', '-1', '5'), '4')
        self.assertEqual(self.s('mods', '4', '6'), '-2')
        self.assertEqual(self.s('mod', '1/3', '7'), '5')
        self.assertEqual(self.s('mod', 'x^2+8*x+3', '5'), 'x^2+3*x+3')

    def test_numtheory_functions(self):
        self.assertEqual(self.s('phi', '12'), '4')
        self.assertEqual(self.s('divisors', '12'), '{1,2,3,4,6,12}')
        self.assertEqual(self.s('binomial', '5', '2'), '10')
        self.assertEqual(self.s('factorial', '5'), '120')

    def test_rounding_functions(self):
        self.assertEqual(self.s('floor', '7/2'), '3')
        self.assertEqual(self.s('ceil', '7/2'), '4')
        self.assertEqual(self.s('round', '5/2'), '3')
        self.assertEqual(self.s('round', '-5/2'), '-3')
        self.assertEqual(self.s('trunc', '-7/2'), '-3')
        self.assertEqual(self.s('frac', '-7/2'), '-1/2')
        self.assertEqual(self.s('floor', '-0.5'), '-1')

    def test_abs_signum_max_min(self):
        self.assertEqual(self.s('abs', '-3'), '3')
        self.assertEqual(self.s('signum', '-3'), '-1')
        self.assertEqual(self.s('max', '1', '5', '3'), '5')
        self.assertEqual(self.s('min', '[4,2,8]'), '2')
        self.assertEqual(self.s('max', 'x', '2'), 'max(2,x)')

    def test_errors(self):
        self.assertIn('numeric exception', self.err('iquo', '1', '0'))
        self.assertIn('wrong number', self.err('isprime', 'x'))


class TestSpecialFunctions(Base):
    def test_gamma_family(self):
        self.assertEqual(self.s('GAMMA', '5'), '24')
        self.assertEqual(self.s('GAMMA', '1/2'), 'sqrt(Pi)')
        self.assertEqual(self.s('Beta', '2', '3'), '1/12')
        self.assertEqual(self.s('Psi', '1'), '-gamma')
        self.assertEqual(self.s('Zeta', '2'), 'Pi^2/6')
        self.assertEqual(self.s('GAMMA', '1/2', 'x'), 'sqrt(Pi)*erfc(sqrt(x))')

    def test_named_functions_stay_symbolic(self):
        self.assertEqual(self.s('BesselJ', '0', 'x'), 'BesselJ(0,x)')
        self.assertEqual(self.s('AiryAi', 'x'), 'AiryAi(x)')
        self.assertEqual(self.s('LambertW', 'x'), 'LambertW(x)')
        self.assertEqual(self.s('LambertW', '0', 'x'), 'LambertW(x)')
        self.assertEqual(self.s('erf', 'x'), 'erf(x)')
        self.assertEqual(self.s('Ei', 'x'), 'Ei(x)')

    def test_numeric_evaluation(self):
        self.assertEqual(self.s('evalf', 'BesselJ(0,1)'), '0.7651976866')
        self.assertEqual(self.s('LambertW', '1.0'), '0.5671432904')
        self.assertEqual(self.s('erf', '0.5'), '0.5204998778')
        self.assertEqual(self.s('Ei', '1.0'), '1.895117816')
        self.assertEqual(self.s('Si', '1.0'), '0.9460830704')
        self.assertEqual(self.s('evalf', 'Zeta(3)', digits=20), '1.2020569031595942854')

    def test_orthogonal_polynomials(self):
        self.assertEqual(self.s('LegendreP', '2', 'x'), '3*x^2/2-1/2')
        self.assertEqual(self.s('HermiteH', '3', 'x'), '8*x^3-12*x')
        self.assertEqual(self.s('LaguerreL', '2', 'x'), 'x^2/2-2*x+1')
        self.assertEqual(self.s('ChebyshevT', '3', 'x'), '4*x^3-3*x')
        self.assertEqual(self.s('ChebyshevU', '2', 'x'), '4*x^2-1')

    def test_dilog_polylog_elliptic(self):
        self.assertEqual(self.s('dilog', '2'), '-Pi^2/12')
        self.assertEqual(self.s('polylog', '2', '1/2'), '-ln(2)^2/2+Pi^2/12')
        self.assertEqual(self.s('EllipticK', 'k'), 'EllipticK(k)')
        self.assertEqual(self.s('evalf', 'EllipticK(1/2)'), '1.685750355')
        self.assertEqual(self.s('hypergeom', '[1,2]', '[3]', 'z'), 'hypergeom([1,2],[3],z)')

    def test_heaviside_dirac_piecewise(self):
        self.assertEqual(self.s('Heaviside', '-1'), '0')
        self.assertEqual(self.s('Heaviside', '2'), '1')
        self.assertEqual(self.s('Dirac', 'x'), 'Dirac(x)')
        self.assertEqual(self.s('piecewise', 'x<0', '-x', 'x'), 'piecewise(x<0,-x,x)')
        self.assertEqual(self.s('subs', 'x=-2', {'fn': 'piecewise', 'args': [X('x<0'), X('-x'), X('x')]}), '2')

    def test_trig_and_hyperbolic(self):
        self.assertEqual(self.s('sin', 'Pi/6'), '1/2')
        self.assertEqual(self.s('arcsin', '1'), 'Pi/2')
        self.assertEqual(self.s('arctan', '1', '1'), 'Pi/4')
        self.assertEqual(self.s('cosh', '0'), '1')
        self.assertEqual(self.s('arctanh', '0'), '0')
        self.assertEqual(self.s('sec', '0'), '1')

    def test_roots_and_logs(self):
        self.assertEqual(self.s('sqrt', '-4'), '2*I')
        self.assertEqual(self.s('root', '8', '3'), '2')
        self.assertEqual(self.s('surd', '-8', '3'), '-2')
        self.assertEqual(self.s('ln', '-1'), 'I*Pi')
        self.assertEqual(self.s('log10', '1000'), '3')
        self.assertEqual(self.s('ln', 'x'), 'ln(x)')
        node = {'fnidx': ['log', [X('2')], [X('8')]]}
        r = engine().send({'op': 'call', 'name': 'simplify', 'args': [node], 'opts': {}})
        self.assertEqual(S(r['result']), '3')

    def test_complex_parts(self):
        self.assertEqual(self.s('abs', '3+4*I'), '5')
        self.assertEqual(self.s('Re', '3+4*I'), '3')
        self.assertEqual(self.s('Im', '3+4*I'), '4')
        self.assertEqual(self.s('conjugate', '3+4*I'), '3-4*I')
        self.assertEqual(self.s('argument', 'I'), 'Pi/2')
        self.assertEqual(self.s('csgn', '-2'), '-1')
        self.assertEqual(self.s('csgn', 'I'), '1')
        self.assertEqual(self.s('signum', 'I'), 'I')

    def test_wrong_number_of_arguments(self):
        self.assertEqual(self.err('sin', 'x', 'y'), '(in sin) wrong number (or type) of arguments')


class TestTransforms(Base):
    def test_laplace_argument_order(self):
        self.assertEqual(self.s('laplace', 'sin(t)', 't', 's'), '1/(s^2+1)')
        self.eqv('laplace', 'exp(a*t)', 't', 's', expect='1/(s-a)')
        self.assertEqual(self.s('laplace', 't^2', 't', 's'), '2/s^3')

    def test_laplace_of_derivative(self):
        self.assertEqual(self.s('laplace', 'diff(y(t),t)', 't', 's'), 's*laplace(y(t),t,s)-y(0)')

    def test_invlaplace(self):
        self.assertEqual(self.s('invlaplace', '1/(s^2+1)', 's', 't'), 'sin(t)')
        self.assertEqual(self.s('invlaplace', '1/(s-a)', 's', 't'), 'exp(a*t)')

    def test_fourier_pair(self):
        self.assertEqual(self.s('fourier', 'exp(-t^2)', 't', 'w'), 'sqrt(Pi)*exp(-w^2/4)')
        self.assertEqual(self.s('invfourier', 'sqrt(Pi)*exp(-w^2/4)', 'w', 't'), 'exp(-t^2)')

    def test_mellin(self):
        self.assertEqual(self.s('mellin', 'exp(-x)', 'x', 's'), 'GAMMA(s)')


class TestVectorCalculus(Base):
    def test_gradient_divergence_curl(self):
        self.assertEqual(self.s('Gradient', 'x^2*y', '[x,y]'), 'Vector[2*x*y,x^2]')
        self.assertEqual(self.s('Divergence', VEC(['x', 'y', 'z']), '[x,y,z]'), '3')
        self.assertEqual(self.s('Curl', VEC(['y', '-x', '0']), '[x,y,z]'), 'Vector[0,0,-2]')

    def test_laplacian_jacobian_hessian(self):
        self.assertEqual(self.s('Laplacian', 'x^2+y^2', '[x,y]'), '4')
        self.assertEqual(self.s('Jacobian', '[x*y,x+y]', '[x,y]'), 'Matrix[[y,x],[1,1]]')
        self.assertEqual(self.s('Hessian', 'x^2*y', '[x,y]'), 'Matrix[[2*y,2*x],[2*x,0]]')

    def test_wronskian(self):
        self.assertEqual(self.s('Wronskian', '[sin(x),cos(x)]', 'x'),
                         'Matrix[[sin(x),cos(x)],[cos(x),-sin(x)]]')


class TestMisc(Base):
    def test_nops_op(self):
        self.assertEqual(self.s('nops', 'x+y+z'), '3')
        self.assertEqual(self.s('op', '2', 'x+y+z'), 'y')
        self.assertEqual(self.s('op', '0', 'x+y+z'), '+')
        self.assertEqual(self.s('op', '0', 'x*y'), '*')
        self.assertEqual(self.s('op', '[a,b,c]'), 'a,b,c')
        self.assertEqual(self.s('op', '2..3', '[a,b,c]'), 'b,c')
        self.assertEqual(self.s('nops', 'sin(x)'), '1')
        self.assertEqual(self.s('op', '1', 'x^2'), 'x')

    def test_lhs_rhs(self):
        self.assertEqual(self.s('lhs', 'x=1'), 'x')
        self.assertEqual(self.s('rhs', 'x=1'), '1')
        self.assertEqual(self.s('rhs', '1..5'), '5')

    def test_indets_and_has(self):
        self.assertEqual(self.s('indets', 'x^2+sin(y)+Pi'), '{x,y,sin(y)}')
        self.assertEqual(self.s('has', 'x^2+y', 'y'), 'true')
        self.assertEqual(self.s('has', 'x^2+y', 'z'), 'false')

    def test_type(self):
        self.assertEqual(self.s('type', '3', 'integer'), 'true')
        self.assertEqual(self.s('type', '3', 'posint'), 'true')
        self.assertEqual(self.s('type', '1/3', 'rational'), 'true')
        self.assertEqual(self.s('type', '1/3', 'integer'), 'false')
        self.assertEqual(self.s('type', '0.5', 'float'), 'true')
        self.assertEqual(self.s('type', 'x', 'symbol'), 'true')
        self.assertEqual(self.s('type', 'x^2+1', 'polynom'), 'true')
        self.assertEqual(self.s('type', '1/x', 'polynom'), 'false')
        self.assertEqual(self.s('type', '[1,2]', 'list'), 'true')
        self.assertEqual(self.s('type', '{1,2}', 'set'), 'true')
        self.assertEqual(self.s('type', 'x=1', 'equation'), 'true')
        self.assertEqual(self.s('type', '2', 'positive'), 'true')
        self.assertEqual(self.s('type', 'Pi', 'constant'), 'true')
        self.assertEqual(self.s('type', MAT([['1']]), 'Matrix'), 'true')
        self.assertEqual(self.s('type', '3', '{integer, float}'), 'true')

    def test_whattype(self):
        self.assertEqual(self.s('whattype', 'x+y'), '+')
        self.assertEqual(self.s('whattype', '1/3'), 'fraction')
        self.assertEqual(self.s('whattype', '3'), 'integer')
        self.assertEqual(self.s('whattype', '[1]'), 'list')
        self.assertEqual(self.s('whattype', '1+2*I'), 'complex')

    def test_unapply_returns_a_proc(self):
        r = self.ok('unapply', 'x^2+y', 'x')
        self.assertEqual(r['proc']['params'], ['x'])
        self.assertEqual(S(r), '(x)->x^2+y')

    def test_isolate(self):
        self.assertEqual(self.s('isolate', '2*x+1=5', 'x'), 'x=2')

    def test_indexed_names_round_trip(self):
        a1 = {'idx': [{'sym': 'a'}, [{'int': '1'}]]}
        a2 = {'idx': [{'sym': 'a'}, [{'int': '2'}]]}
        r = engine().send({'op': 'call', 'name': 'expand', 'args': [{'pow': [{'add': [a1, a2]}, {'int': '2'}]}],
                           'opts': {}})
        self.assertEqual(S(r['result']), 'a[1]^2+2*a[1]*a[2]+a[2]^2')
        r = engine().send({'op': 'call', 'name': 'solve',
                           'args': [{'eq': [{'add': [a1, a2]}, {'int': '3'}]}, a1], 'opts': {}})
        self.assertEqual(S(r['result']), '3-a[2]')

    def test_strings_and_booleans_pass_through(self):
        r = engine().send({'op': 'call', 'name': 'mystery', 'args': [{'str': 'hi'}, {'bool': True}, {'fail': True}],
                           'opts': {}})
        self.assertEqual(S(r['result']), 'mystery("hi",true,FAIL)')

    def test_logic_nodes(self):
        r = engine().send({'op': 'call', 'name': 'evalb', 'args': [{'and': [X('1<2'), X('2<3')]}], 'opts': {}})
        self.assertEqual(r['result'], {'bool': True})
        r = engine().send({'op': 'call', 'name': 'evalb', 'args': [{'not': X('1<2')}], 'opts': {}})
        self.assertEqual(r['result'], {'bool': False})


class TestVerify(Base):
    def test_equal_symbolic(self):
        r = self.verify('(x+1)^2', 'x^2+2*x+1')
        self.assertEqual((r['verdict'], r['method']), ('equal', 'symbolic'))
        self.assertEqual(r['difference'], {'int': '0'})

    def test_equal_symbolic_trig(self):
        r = self.verify('sin(2*x)', '2*sin(x)*cos(x)')
        self.assertEqual((r['verdict'], r['method']), ('equal', 'symbolic'))

    def test_equal_numeric_for_floats(self):
        r = self.verify('0.7071067812', 'sqrt(2)/2')
        self.assertEqual((r['verdict'], r['method']), ('equal', 'numeric'))

    def test_equal_numeric_hard_identity(self):
        r = self.verify('cos(Pi/7)+cos(3*Pi/7)+cos(5*Pi/7)', '1/2')
        self.assertEqual(r['verdict'], 'equal')

    def test_not_equal_with_counterexample(self):
        r = self.verify('x^2', 'x^3')
        self.assertEqual(r['verdict'], 'not_equal')
        self.assertEqual(r['method'], 'numeric')
        self.assertIn('x', r['counterexample'])
        x = float(r['counterexample']['x'])
        self.assertNotAlmostEqual(x ** 2, x ** 3)

    def test_counterexample_respects_branches(self):
        r = self.verify('sqrt(x^2)', 'x')
        self.assertEqual(r['verdict'], 'not_equal')
        self.assertLess(float(r['counterexample']['x']), 0)

    def test_assumptions_turn_not_equal_into_equal(self):
        r = self.verify('sqrt(x^2)', 'x', assume=['0<x'])
        self.assertEqual(r['verdict'], 'equal')
        r = self.verify('ln(x*y)', 'ln(x)+ln(y)')
        self.assertEqual(r['verdict'], 'not_equal')
        r = self.verify('ln(x*y)', 'ln(x)+ln(y)', assume=['0<x', '0<y'])
        self.assertEqual(r['verdict'], 'equal')

    def test_counterexample_is_consistent_with_assumptions(self):
        r = self.verify('x^2', 'x^3', assume=['0<x'])
        self.assertEqual(r['verdict'], 'not_equal')
        self.assertGreater(float(r['counterexample']['x']), 0)

    def test_integer_assumption(self):
        r = self.verify('(-1)^(2*n)', '1', assume=[TYPE('n', 'integer')])
        self.assertEqual(r['verdict'], 'equal')

    def test_unknown_functions(self):
        self.assertEqual(self.verify('f(x)', 'g(x)')['verdict'], 'not_equal')
        self.assertEqual(self.verify('f(x)', 'f(x)')['verdict'], 'equal')
        self.assertEqual(self.verify('f(x)+1', '1+f(x)')['verdict'], 'equal')

    def test_undetermined_when_nothing_can_be_evaluated(self):
        r = self.verify('Int(f(x),x=0..1)', 'Int(g(x),x=0..1)')
        self.assertEqual(r['verdict'], 'undetermined')
        r = self.verify('Int(f(x),x=0..1)', 'Int(f(1-x),x=0..1)')
        self.assertEqual(r['verdict'], 'undetermined')

    def test_lists_are_elementwise(self):
        self.assertEqual(self.verify('[x+x,2*y]', '[2*x,y*2]')['verdict'], 'equal')
        r = self.verify('[x,y]', '[x,z]')
        self.assertEqual(r['verdict'], 'not_equal')
        self.assertEqual(self.verify('[1,2]', '[1,2,3]')['verdict'], 'not_equal')

    def test_sets_ignore_order(self):
        self.assertEqual(self.verify('{x=2,y=1}', '{y=1,x=2}')['verdict'], 'equal')
        self.assertEqual(self.verify('{1,2}', '{1,3}')['verdict'], 'not_equal')

    def test_equations_scaled_are_equivalent(self):
        self.assertEqual(self.verify('x^2-4=0', 'x^2=4')['verdict'], 'equal')
        self.assertEqual(self.verify('2*x+2=0', 'x+1=0')['verdict'], 'equal')
        self.assertEqual(self.verify('x^2=4', 'x^2=5')['verdict'], 'not_equal')

    def test_matrices(self):
        a = MAT([['x+x', '1'], ['0', '1']])
        b = MAT([['2*x', '1'], ['0', '1']])
        c = MAT([['2*x', '1'], ['1', '1']])
        self.assertEqual(self.verify(a, b)['verdict'], 'equal')
        self.assertEqual(self.verify(a, c)['verdict'], 'not_equal')

    def test_different_kinds_are_not_equal(self):
        self.assertEqual(self.verify('[1]', '1')['verdict'], 'not_equal')

    def test_gamma_identity(self):
        self.assertEqual(self.verify('GAMMA(x+1)', 'x*GAMMA(x)')['verdict'], 'equal')

    def test_derivative_check(self):
        self.assertEqual(self.verify('diff(sin(x)*x,x)', 'x*cos(x)+sin(x)')['verdict'], 'equal')

    def test_never_raises_for_math_reasons(self):
        r = self.verify('1/0', '1')
        self.assertIn(r['verdict'], ('undetermined', 'not_equal'))
        self.assertEqual(self.verify('x/x', '1')['verdict'], 'equal')

    def test_bad_expression_is_an_error_not_a_crash(self):
        r = engine().send({'op': 'verify', 'lhs': {'bogus': 1}, 'rhs': X('1'), 'opts': {}})
        self.assertFalse(r['ok'])
        self.assertTrue(engine().send({'op': 'ping'})['ok'])

    def test_difference_is_reported_when_not_equal(self):
        r = self.verify('x+1', 'x+2')
        self.assertEqual(S(r['difference']), '-1')


class TestPrinters(Base):
    def latex(self, expr):
        r = engine().send({'op': 'latex', 'expr': X(expr)})
        self.assertTrue(r['ok'], r)
        return r['result']

    def pretty(self, expr, width=80):
        r = engine().send({'op': 'pretty', 'expr': X(expr), 'width': width})
        self.assertTrue(r['ok'], r)
        return r['result']

    def test_latex_basic(self):
        self.assertEqual(self.latex('x^3/3'), '\\frac{x^{3}}{3}')

    def test_latex_uses_ln_not_log(self):
        out = self.latex('ln(x)')
        self.assertIn('\\ln', out)
        self.assertNotIn('\\log', out)

    def test_latex_exp_one_is_e(self):
        self.assertEqual(self.latex('exp(1)'), 'e')

    def test_latex_gamma_and_arcsin(self):
        self.assertIn('\\Gamma', self.latex('GAMMA(x)'))
        self.assertIn('\\arcsin', self.latex('arcsin(x)'))

    def test_latex_structures(self):
        self.assertEqual(self.latex('[1,2,x]'), '\\left[ 1, 2, x \\right]')
        self.assertEqual(self.latex('{x=1,y=2}'), '\\left\\{ x = 1, y = 2 \\right\\}')
        self.assertIn('matrix', self.latex(MAT([['1', '2'], ['3', '4']])))

    def test_pretty_fraction_is_two_dimensional(self):
        out = self.pretty('x^2/(x+1)')
        self.assertGreater(len(out.splitlines()), 1)
        self.assertIn('x + 1', out)

    def test_pretty_ln_and_e(self):
        out = self.pretty('ln(x)*exp(x)')
        self.assertIn('ln(x)', out)
        self.assertNotIn('log', out)
        self.assertIn('e', out)
        self.assertNotIn('ℯ', out)

    def test_pretty_imaginary_unit_is_I(self):
        self.assertIn('I', self.pretty('2+3*I'))

    def test_pretty_sets_and_equations(self):
        self.assertEqual(self.pretty('{x = 1, y = 2}'), '{x = 1, y = 2}')

    def test_pretty_width_wraps(self):
        long = '+'.join('a{}'.format(i) for i in range(40))
        out = self.pretty(long, width=40)
        self.assertGreater(len(out.splitlines()), 1)
        self.assertTrue(all(len(line) <= 45 for line in out.splitlines()))

    def test_pretty_matrix(self):
        out = self.pretty(MAT([['1', '2'], ['3', '4']]))
        self.assertGreaterEqual(len(out.splitlines()), 2)


REGISTRY_NAMES = """simplify expand factor normal combine collect sort rationalize radnormal evalc evalf evalb is testeq
convert coeff coeffs degree ldegree lcoeff tcoeff numer denom quo rem gcd lcm resultant discrim content primpart sqrfree
roots RootOf allvalues Basis interp ifactor ifactors isprime ithprime nextprime prevprime igcd ilcm iquo irem mod modp
mods phi divisors binomial factorial floor ceil round trunc frac abs signum max min diff D int Int value limit series
taylor mtaylor sum product Sum Product residue singular implicitdiff extrema maximize minimize solve fsolve isolve rsolve
dsolve pdsolve odetest Matrix Vector Determinant MatrixInverse Transpose HermitianTranspose Eigenvalues Eigenvectors Rank
Trace NullSpace ColumnSpace RowSpace LinearSolve ReducedRowEchelonForm GaussianElimination LUDecomposition
QRDecomposition CharacteristicPolynomial MinimalPolynomial JordanForm DotProduct CrossProduct Norm Normalize
IdentityMatrix ZeroMatrix DiagonalMatrix MatrixExponential MatrixPower IsDefinite GramSchmidt SingularValues laplace
invlaplace fourier invfourier mellin Gradient Divergence Curl Laplacian Jacobian Hessian Wronskian GAMMA Beta Psi Zeta erf
erfc Ei Si Ci LambertW BesselJ BesselY BesselI BesselK AiryAi AiryBi LegendreP HermiteH LaguerreL ChebyshevT ChebyshevU
hypergeom EllipticK EllipticE EllipticF dilog polylog Heaviside Dirac piecewise arcsin arccos arctan arccot arcsec arccsc
sinh cosh tanh coth sech csch arcsinh arccosh arctanh arccoth arcsech arccsch ln log10 exp sqrt surd root csgn argument
Re Im conjugate nops op lhs rhs indets has type whattype subs eval unapply sin cos tan cot sec csc""".split()


class TestRegistry(Base):
    def test_every_registry_name_is_known(self):
        unknown = []
        for name in REGISTRY_NAMES:
            r = engine().send({'op': 'call', 'name': name, 'args': [], 'opts': {}})
            if r.get('ok') and r['result'] == {'fn': name, 'args': []}:
                unknown.append(name)
        self.assertEqual(unknown, [])

    def test_unknown_names_are_echoed_back(self):
        r = engine().send({'op': 'call', 'name': 'NoSuchFunction', 'args': [X('x')], 'opts': {}})
        self.assertEqual(r['result'], {'fn': 'NoSuchFunction', 'args': [X('x')]})

    def test_startup_is_fast(self):
        import time
        t0 = time.time()
        e = EngineProc()
        e.send({'op': 'ping'})
        elapsed = time.time() - t0
        e.close()
        self.assertLess(elapsed, 5.0)


if __name__ == '__main__':
    unittest.main()
