##   AlgebraKing ##
##
##   File:         [engine.py]
##
##   Author(s):    ['Pedro H.F Matias']
##   Site(s):      ['https://github.com/phfmatias']
##   Email(s):     ['pedrohfmatias@gmail.com']
##   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
##   Date:         ['27.10.2026']
##   Version:      ['0.0.1']
##   Status:       ['Development']
##   Language:     ['Python']
##   Description:  ['SymPy math engine speaking JSON lines for AlgebraKing, with Maple function names and semantics.']

import json
import math
import os
import random
import re
import signal
import sys
import time
import traceback
import warnings

warnings.filterwarnings('ignore')
DEBUG = bool(os.environ.get('ALGEBRAKING_DEBUG'))
sys.setrecursionlimit(9000)

import mpmath
import sympy as sp
from mpmath.libmp import prec_to_dps, to_str
from sympy.core.facts import InconsistentAssumptions
from sympy.printing.latex import LatexPrinter
from sympy.printing.pretty.pretty import PrettyPrinter


# ----------------------------------------------------------------------------
# Small value classes (Maple structures that SymPy has no direct twin for)
# ----------------------------------------------------------------------------

class MapleError(Exception):
    """Error that is shown to the user as Maple would show it."""


class EngineTimeout(BaseException):
    """Raised by the alarm handler; BaseException so SymPy cannot swallow it."""


class Fail(object):
    def __repr__(self):
        return 'FAIL'


FAIL = Fail()


class Str(str):
    """Maple string."""


class Seq(list):
    """Expression sequence."""


class MList(list):
    """Maple list."""


class MSet(list):
    """Maple set (duplicates removed, printed sorted)."""


class Rng(object):
    """Range a..b."""

    def __init__(self, lo, hi):
        self.lo = lo
        self.hi = hi


class Mat(object):
    """Matrix or Vector; kind is 'matrix', 'col' or 'row'."""

    def __init__(self, m, kind='matrix'):
        self.m = sp.Matrix(m)
        self.kind = kind


class RelObj(object):
    """Relation whose sides are not SymPy objects (lists, matrices...)."""

    def __init__(self, op, lhs, rhs):
        self.op = op
        self.lhs = lhs
        self.rhs = rhs


class Raw(object):
    """Already-built EXPR node, passed through untouched."""

    def __init__(self, node):
        self.node = node


class Wrap(sp.Atom):
    """Holds a non-SymPy object (list, matrix, string) as an argument of an unevaluated function."""
    is_Atom = True

    def __new__(cls, obj):
        o = sp.Basic.__new__(cls)
        o.obj = obj
        return o

    def _hashable_content(self):
        return (id(self.obj),)

    @property
    def free_symbols(self):
        return set()


class IdxSym(sp.Symbol):
    """Indexed name a[1,2] kept as one symbol that remembers its EXPR parts."""
    _idxBase = None
    _idxArgs = None


def mkSet(items):
    out = MSet()
    for it in items:
        if not any(sameObj(it, o) for o in out):
            out.append(it)
    return out


def sameObj(a, b):
    try:
        return bool(a == b)
    except (TypeError, ValueError):
        return False


def isSym(x):
    return isinstance(x, sp.Symbol)


def isRel(x):
    return isinstance(x, (sp.core.relational.Relational, RelObj))


def relParts(x):
    """Return (op, lhs, rhs) with op in eq, neq, lt, le (gt/ge are swapped)."""
    if isinstance(x, RelObj):
        return x.op, x.lhs, x.rhs
    op = x.rel_op
    if op == '==':
        return 'eq', x.lhs, x.rhs
    if op == '!=':
        return 'neq', x.lhs, x.rhs
    if op == '<':
        return 'lt', x.lhs, x.rhs
    if op == '<=':
        return 'le', x.lhs, x.rhs
    if op == '>':
        return 'lt', x.rhs, x.lhs
    return 'le', x.rhs, x.lhs


def mkRel(op, lhs, rhs):
    if isinstance(lhs, sp.Basic) and isinstance(rhs, sp.Basic):
        cls = {'eq': sp.Eq, 'neq': sp.Ne, 'lt': sp.Lt, 'le': sp.Le}[op]
        try:
            return cls(lhs, rhs, evaluate=False)
        except TypeError:
            pass
    return RelObj(op, lhs, rhs)


def mkEq(lhs, rhs):
    return mkRel('eq', lhs, rhs)


def asExpr(x):
    """Equation -> lhs - rhs, anything else unchanged."""
    if isRel(x):
        op, l, r = relParts(x)
        if op == 'eq':
            return l - r
    return x


def mapS(fn, x):
    """Apply fn to every component of a Maple structure (list, set, equation, matrix)."""
    if isinstance(x, Mat):
        return Mat(x.m.applyfunc(fn), x.kind)
    if isinstance(x, MSet):
        return mkSet([mapS(fn, i) for i in x])
    if isinstance(x, (MList, Seq)):
        return type(x)([mapS(fn, i) for i in x])
    if isRel(x):
        op, l, r = relParts(x)
        return mkRel(op, mapS(fn, l), mapS(fn, r))
    if isinstance(x, Rng):
        return Rng(mapS(fn, x.lo), mapS(fn, x.hi))
    return fn(x)


def isNum(x):
    return isinstance(x, sp.Basic) and x.is_number


def intOf(x, what='argument'):
    try:
        if isinstance(x, sp.Basic) and x.is_Integer:
            return int(x)
        if isinstance(x, int):
            return x
    except (TypeError, ValueError):
        pass
    raise MapleError('wrong number (or type) of arguments')


def timeLimited(seconds, fn, *args):
    """Run fn with an alarm; returns (ok, value). Never raises for a timeout."""
    def onAlarm(signum, frame):
        raise EngineTimeout()
    old = None
    try:
        old = signal.signal(signal.SIGALRM, onAlarm)
        signal.setitimer(signal.ITIMER_REAL, seconds)
    except (ValueError, OSError):
        old = None
    try:
        return True, fn(*args)
    except EngineTimeout:
        return False, None
    except RecursionError:
        return False, None
    except Exception:
        return False, None
    finally:
        try:
            signal.setitimer(signal.ITIMER_REAL, 0)
            if old is not None:
                signal.signal(signal.SIGALRM, old)
        except (ValueError, OSError):
            pass


# ----------------------------------------------------------------------------
# Maple special functions that SymPy lacks
# ----------------------------------------------------------------------------

class Csgn(sp.Function):
    """Maple csgn: complex sign (1 if Re>0 or Re=0 and Im>0)."""
    nargs = 1

    @classmethod
    def eval(cls, z):
        if z.is_number:
            re_, im_ = z.as_real_imag()
            if re_.is_positive or (re_.is_zero and im_.is_positive):
                return sp.Integer(1)
            if re_.is_negative or (re_.is_zero and im_.is_negative):
                return sp.Integer(-1)
            if z.is_zero:
                return sp.Integer(0)
        if z.is_positive:
            return sp.Integer(1)
        if z.is_negative:
            return sp.Integer(-1)
        if z.is_zero:
            return sp.Integer(0)
        return None


Csgn.__name__ = 'csgn'


# ----------------------------------------------------------------------------
# Assumptions (the engine is stateless: they arrive in every request)
# ----------------------------------------------------------------------------

TYPE_PROPS = {
    'integer': {'integer': True},
    'posint': {'integer': True, 'positive': True},
    'negint': {'integer': True, 'negative': True},
    'nonnegint': {'integer': True, 'nonnegative': True},
    'nonposint': {'integer': True, 'nonpositive': True},
    'natural': {'integer': True, 'positive': True},
    'rational': {'rational': True},
    'real': {'real': True},
    'realcons': {'real': True},
    'positive': {'positive': True},
    'negative': {'negative': True},
    'nonneg': {'nonnegative': True},
    'nonnegative': {'nonnegative': True},
    'nonpositive': {'nonpositive': True},
    'complex': {},
    'even': {'even': True},
    'odd': {'odd': True},
    'prime': {'prime': True},
    'nonzero': {'nonzero': True},
    'imaginary': {'imaginary': True},
}


class Assumptions(object):
    """Turns the session's assume list into SymPy symbols with assumptions and sampling info."""

    def __init__(self, items):
        self.props = {}
        self.bounds = {}
        self.cache = {}
        plain = Convert(None, 10)
        self.plain = plain
        for it in items or []:
            try:
                self.addNode(it)
            except (MapleError, KeyError, TypeError, ValueError, IndexError):
                continue
        self.finish()

    def symName(self, node):
        if isinstance(node, dict) and 'sym' in node:
            return node['sym']
        return None

    def numVal(self, node):
        try:
            v = self.plain.toSympy(node)
        except (MapleError, KeyError, TypeError, ValueError, IndexError):
            return None
        if isinstance(v, sp.Basic) and v.is_number and v.is_real:
            return v
        return None

    def addProps(self, name, props):
        self.props.setdefault(name, {}).update(props)

    def addBound(self, name, side, val, strict):
        b = self.bounds.setdefault(name, {})
        b[side] = (val, strict)

    def addNode(self, node):
        if not isinstance(node, dict):
            return
        for key in ('and',):
            if key in node:
                for sub in node[key]:
                    self.addNode(sub)
                return
        for key in ('set', 'list', 'seq'):
            if key in node:
                for sub in node[key]:
                    self.addNode(sub)
                return
        if 'fn' in node and node['fn'] == 'type' and len(node.get('args', [])) == 2:
            who, what = node['args']
            names = []
            if isinstance(who, dict) and 'sym' in who:
                names = [who['sym']]
            elif isinstance(who, dict) and 'set' in who:
                names = [self.symName(w) for w in who['set']]
            tname = what.get('sym') if isinstance(what, dict) else None
            if tname in TYPE_PROPS:
                for n in names:
                    if n:
                        self.addProps(n, TYPE_PROPS[tname])
            return
        for op in ('lt', 'le'):
            if op in node:
                a, b = node[op]
                strict = (op == 'lt')
                sa, sb = self.symName(a), self.symName(b)
                if sb and sa is None:
                    v = self.numVal(a)
                    if v is not None:
                        self.addBound(sb, 'lo', v, strict)
                    else:
                        self.addProps(sb, {'real': True})
                elif sa and sb is None:
                    v = self.numVal(b)
                    if v is not None:
                        self.addBound(sa, 'hi', v, strict)
                    else:
                        self.addProps(sa, {'real': True})
                elif sa and sb:
                    self.addProps(sa, {'real': True})
                    self.addProps(sb, {'real': True})
                return
        if 'neq' in node:
            a, b = node['neq']
            sa, sb = self.symName(a), self.symName(b)
            if sa and self.numVal(b) == 0:
                self.addProps(sa, {'nonzero': True})
            elif sb and self.numVal(a) == 0:
                self.addProps(sb, {'nonzero': True})
            return
        if 'not' in node and isinstance(node['not'], dict) and 'eq' in node['not']:
            self.addNode({'neq': node['not']['eq']})

    def finish(self):
        for name, b in self.bounds.items():
            p = self.props.setdefault(name, {})
            lo, hi = b.get('lo'), b.get('hi')
            if lo is not None:
                v, strict = lo
                if v > 0 or (v == 0 and strict):
                    p['positive'] = True
                elif v == 0:
                    p['nonnegative'] = True
                else:
                    p['real'] = True
            if hi is not None:
                v, strict = hi
                if v < 0 or (v == 0 and strict):
                    p['negative'] = True
                elif v == 0:
                    p['nonpositive'] = True
                else:
                    p['real'] = True
            if lo is not None and hi is not None:
                p['real'] = True
                p.pop('positive', None)
                p.pop('negative', None)
                p.pop('nonnegative', None)
                p.pop('nonpositive', None)
                lov, loS = lo
                hiv, hiS = hi
                if lov > 0 or (lov == 0 and loS):
                    p['positive'] = True
                elif lov == 0:
                    p['nonnegative'] = True
                if hiv < 0 or (hiv == 0 and hiS):
                    p['negative'] = True
                elif hiv == 0:
                    p['nonpositive'] = True

    def symbol(self, name):
        s = self.cache.get(name)
        if s is None:
            props = self.props.get(name)
            if props:
                try:
                    s = sp.Symbol(name, **props)
                except (ValueError, TypeError, InconsistentAssumptions):
                    s = sp.Symbol(name)
            else:
                s = sp.Symbol(name)
            self.cache[name] = s
        return s

    def sampling(self, name):
        """Info used by the numeric verifier to sample a symbol."""
        p = self.props.get(name, {})
        b = self.bounds.get(name, {})
        return p, b


# ----------------------------------------------------------------------------
# EXPR <-> SymPy
# ----------------------------------------------------------------------------

PACKAGES = {'LinearAlgebra', 'inttrans', 'student', 'VectorCalculus', 'numtheory', 'Groebner',
            'Student', 'Calculus1', 'combinat', 'stats', 'plots', 'DEtools', 'PDEtools',
            'MatrixPolynomialAlgebra', 'Statistics', 'ListTools', 'polytools'}


def shortName(name):
    if not isinstance(name, str):
        return name
    if ':-' in name:
        name = name.split(':-')[-1]
    m = re.match(r'^(\w+)\[(\w+)\]$', name)
    if m and m.group(1) in PACKAGES:
        return m.group(2)
    return name


def floatContagion(r):
    """Function calls with a float argument evaluate numerically, as in Maple."""
    if isinstance(r, sp.Basic) and not isinstance(r, sp.Float) and r.is_number and r.has(sp.Float) \
            and not r.has(sp.zoo, sp.nan):
        dps = max(prec_to_dps(f._prec) for f in r.atoms(sp.Float))
        try:
            return r.evalf(dps)
        except (TypeError, ValueError, NotImplementedError):
            return r
    return r


def logBase(x, b):
    """log_b(x); numeric rational arguments are reduced so that log10(1000) gives 3."""
    if x == 0:
        raise MapleError('numeric exception: division by zero')
    if isinstance(x, sp.Rational) and isinstance(b, sp.Rational) and x > 0 and b > 0:
        num = sp.expand_log(sp.log(x), force=True)
        den = sp.expand_log(sp.log(b), force=True)
        return sp.simplify(num / den)
    return sp.log(x) / sp.log(b)


def lnFn(x, b=None):
    if x == 0:
        raise MapleError('numeric exception: division by zero')
    if b is not None:
        return logBase(x, b)
    return sp.log(x)


def zetaFn(*a):
    if len(a) == 1:
        return sp.zeta(a[0])
    if len(a) == 2:
        if a[0] == 0:
            return sp.zeta(a[1])
        if isinstance(a[0], sp.Integer) and a[0] > 0:
            s = sp.Dummy('s')
            return sp.diff(sp.zeta(s), (s, int(a[0]))).subs(s, a[1])
        return sp.zeta(a[0], a[1])
    if len(a) == 3:
        s = sp.Dummy('s')
        return sp.diff(sp.zeta(s, a[2]), (s, int(a[0]))).subs(s, a[1])
    raise MapleError('wrong number (or type) of arguments')


def lambertFn(*a):
    if len(a) == 1:
        return sp.LambertW(a[0])
    return sp.LambertW(a[1], a[0])


def airyFn(plain, prime):
    def f(*a):
        if len(a) == 1:
            return plain(a[0])
        if a[0] == 0:
            return plain(a[1])
        if a[0] == 1:
            return prime(a[1])
        raise MapleError('wrong number (or type) of arguments')
    return f


def eiFn(*a):
    if len(a) == 1:
        return sp.Ei(a[0])
    return sp.expint(a[0], a[1])


def gammaFn(*a):
    if len(a) == 1:
        return sp.gamma(a[0])
    return sp.uppergamma(a[0], a[1])


def psiFn(*a):
    if len(a) == 1:
        return sp.polygamma(0, a[0])
    return sp.polygamma(a[0], a[1])


def legendreFn(*a):
    if len(a) == 2:
        return sp.legendre(a[0], a[1])
    return sp.assoc_legendre(a[0], a[1], a[2])


def laguerreFn(*a):
    if len(a) == 2:
        return sp.laguerre(a[0], a[1])
    return sp.assoc_laguerre(a[0], a[1], a[2])


def ellipKFn(k):
    return sp.elliptic_k(k ** 2)


def ellipEFn(*a):
    if len(a) == 1:
        return sp.elliptic_e(a[0] ** 2)
    return sp.elliptic_e(a[0], a[1] ** 2)


def ellipFFn(z, k):
    return sp.elliptic_f(z, k ** 2)


def ellipPiFn(*a):
    if len(a) == 2:
        return sp.elliptic_pi(a[0], a[1] ** 2)
    return sp.elliptic_pi(a[0], a[1], a[2] ** 2)


def hypergeomFn(a, b, z):
    return sp.hyper(list(a), list(b), z)


def arctanFn(*a):
    if len(a) == 1:
        return sp.atan(a[0])
    return sp.atan2(a[0], a[1])


def piecewiseFn(*a):
    args = list(a)
    pairs = []
    i = 0
    while i + 1 < len(args):
        cond = args[i]
        # conditions arrive unevaluated (mkRel); let SymPy decide numeric ones
        if isinstance(cond, sp.core.relational.Relational):
            cond = cond.func(cond.lhs, cond.rhs)
        pairs.append((args[i + 1], cond))
        i += 2
    if len(args) % 2 == 1:
        pairs.append((args[-1], True))
    else:
        pairs.append((sp.Integer(0), True))
    return sp.Piecewise(*pairs)


def roundFn(x):
    if isNum(x):
        if x.is_real is False:
            raise MapleError('wrong number (or type) of arguments')
        return sp.sign(x) * sp.floor(sp.Abs(x) + sp.Rational(1, 2))
    return sp.Function('round')(x)


def truncFn(x):
    if isNum(x):
        return sp.sign(x) * sp.floor(sp.Abs(x))
    return sp.Function('trunc')(x)


def fracFn(x):
    if isNum(x):
        return x - sp.sign(x) * sp.floor(sp.Abs(x))
    return sp.Function('frac')(x)


def maxFn(*a):
    items = flatItems(a)
    return sp.Max(*items)


def minFn(*a):
    items = flatItems(a)
    return sp.Min(*items)


def flatItems(a):
    out = []
    for x in a:
        if isinstance(x, (MList, MSet, Seq)):
            out.extend(flatItems(x))
        else:
            out.append(x)
    return out


def rootFn(x, n):
    return sp.root(x, n)


def diffFn(f, *vs):
    if isinstance(f, (MList, MSet, Seq, Mat)):
        return mapS(lambda e: diffFn(e, *vs), f)
    flat = []
    for v in vs:
        if isinstance(v, tuple):
            flat.extend([v[0]] * v[1])
        else:
            flat.append(v)
    for v in flat:
        if not isinstance(v, sp.Basic) or not (isSym(v) or isinstance(v, sp.core.function.AppliedUndef)):
            raise MapleError('invalid differentiation variable')
    return sp.diff(f, *flat) if flat else f


def sqrtFn(x):
    return sp.sqrt(x)


def surdFn(x, n):
    return sp.real_root(x, n)


def orderFn(*a):
    return sp.Order(*a)


def buildFuncs():
    F = {
        'sin': sp.sin, 'cos': sp.cos, 'tan': sp.tan, 'cot': sp.cot, 'sec': sp.sec, 'csc': sp.csc,
        'sinh': sp.sinh, 'cosh': sp.cosh, 'tanh': sp.tanh, 'coth': sp.coth, 'sech': sp.sech, 'csch': sp.csch,
        'arcsin': sp.asin, 'arccos': sp.acos, 'arctan': arctanFn, 'arccot': sp.acot, 'arcsec': sp.asec,
        'arccsc': sp.acsc, 'arcsinh': sp.asinh, 'arccosh': sp.acosh, 'arctanh': sp.atanh,
        'arccoth': sp.acoth, 'arcsech': sp.asech, 'arccsch': sp.acsch,
        'exp': sp.exp, 'ln': lnFn, 'sqrt': sqrtFn, 'root': rootFn, 'surd': surdFn,
        'log10': lambda x: logBase(x, sp.Integer(10)), 'log2': lambda x: logBase(x, sp.Integer(2)),
        'abs': sp.Abs, 'signum': sp.sign, 'csgn': Csgn, 'argument': sp.arg, 'Re': sp.re, 'Im': sp.im,
        'conjugate': sp.conjugate,
        'GAMMA': gammaFn, 'Beta': sp.beta, 'Psi': psiFn, 'Zeta': zetaFn,
        'erf': sp.erf, 'erfc': sp.erfc, 'erfi': sp.erfi, 'erfinv': sp.erfinv,
        'Ei': eiFn, 'Si': sp.Si, 'Ci': sp.Ci, 'Shi': sp.Shi, 'Chi': sp.Chi, 'Li': sp.Li,
        'FresnelS': sp.fresnels, 'FresnelC': sp.fresnelc,
        'LambertW': lambertFn,
        'BesselJ': sp.besselj, 'BesselY': sp.bessely, 'BesselI': sp.besseli, 'BesselK': sp.besselk,
        'AiryAi': airyFn(sp.airyai, sp.airyaiprime), 'AiryBi': airyFn(sp.airybi, sp.airybiprime),
        'LegendreP': legendreFn, 'HermiteH': sp.hermite, 'LaguerreL': laguerreFn,
        'ChebyshevT': sp.chebyshevt, 'ChebyshevU': sp.chebyshevu, 'GegenbauerC': sp.gegenbauer,
        'JacobiP': sp.jacobi,
        'hypergeom': hypergeomFn, 'EllipticK': ellipKFn, 'EllipticE': ellipEFn, 'EllipticF': ellipFFn,
        'EllipticPi': ellipPiFn,
        'dilog': lambda x: sp.polylog(2, 1 - x), 'polylog': sp.polylog,
        'Heaviside': sp.Heaviside, 'Dirac': sp.DiracDelta, 'piecewise': piecewiseFn,
        'binomial': sp.binomial, 'factorial': sp.factorial, 'doublefactorial': sp.factorial2,
        'pochhammer': sp.rf, 'fibonacci': sp.fibonacci, 'lucas': sp.lucas, 'bernoulli': sp.bernoulli,
        'euler': sp.euler, 'harmonic': sp.harmonic,
        'floor': sp.floor, 'ceil': sp.ceiling, 'round': roundFn, 'trunc': truncFn, 'frac': fracFn,
        'max': maxFn, 'min': minFn,
        'diff': diffFn, 'O': orderFn,
    }
    return F


FUNCS = buildFuncs()


def buildRev():
    R = {
        sp.sin: 'sin', sp.cos: 'cos', sp.tan: 'tan', sp.cot: 'cot', sp.sec: 'sec', sp.csc: 'csc',
        sp.sinh: 'sinh', sp.cosh: 'cosh', sp.tanh: 'tanh', sp.coth: 'coth', sp.sech: 'sech',
        sp.csch: 'csch', sp.asin: 'arcsin', sp.acos: 'arccos', sp.atan: 'arctan', sp.acot: 'arccot',
        sp.asec: 'arcsec', sp.acsc: 'arccsc', sp.asinh: 'arcsinh', sp.acosh: 'arccosh',
        sp.atanh: 'arctanh', sp.acoth: 'arccoth', sp.asech: 'arcsech', sp.acsch: 'arccsch',
        sp.atan2: 'arctan', sp.exp: 'exp', sp.log: 'ln', sp.Abs: 'abs', sp.sign: 'signum',
        Csgn: 'csgn', sp.arg: 'argument', sp.re: 'Re', sp.im: 'Im', sp.conjugate: 'conjugate',
        sp.gamma: 'GAMMA', sp.uppergamma: 'GAMMA', sp.beta: 'Beta', sp.zeta: 'Zeta',
        sp.erf: 'erf', sp.erfc: 'erfc', sp.erfi: 'erfi', sp.erfinv: 'erfinv',
        sp.Ei: 'Ei', sp.Si: 'Si', sp.Ci: 'Ci', sp.Shi: 'Shi', sp.Chi: 'Chi', sp.Li: 'Li',
        sp.fresnels: 'FresnelS', sp.fresnelc: 'FresnelC',
        sp.besselj: 'BesselJ', sp.bessely: 'BesselY', sp.besseli: 'BesselI', sp.besselk: 'BesselK',
        sp.airyai: 'AiryAi', sp.airybi: 'AiryBi', sp.legendre: 'LegendreP',
        sp.assoc_legendre: 'LegendreP', sp.hermite: 'HermiteH', sp.laguerre: 'LaguerreL',
        sp.assoc_laguerre: 'LaguerreL', sp.chebyshevt: 'ChebyshevT', sp.chebyshevu: 'ChebyshevU',
        sp.gegenbauer: 'GegenbauerC', sp.jacobi: 'JacobiP', sp.polylog: 'polylog',
        sp.Heaviside: 'Heaviside', sp.DiracDelta: 'Dirac', sp.binomial: 'binomial',
        sp.factorial: 'factorial', sp.factorial2: 'doublefactorial', sp.rf: 'pochhammer',
        sp.fibonacci: 'fibonacci', sp.lucas: 'lucas', sp.bernoulli: 'bernoulli', sp.euler: 'euler',
        sp.harmonic: 'harmonic', sp.floor: 'floor', sp.ceiling: 'ceil', sp.Max: 'max', sp.Min: 'min',
        sp.totient: 'phi', sp.divisor_sigma: 'sigma', sp.divisor_count: 'tau',
        sp.LaplaceTransform: 'laplace', sp.InverseLaplaceTransform: 'invlaplace',
        sp.FourierTransform: 'fourier', sp.InverseFourierTransform: 'invfourier',
        sp.MellinTransform: 'mellin', sp.Order: 'O',
    }
    return R


REV = buildRev()


def unsq(m):
    """sqrt(m) with sqrt(k^2) shown as k (Maple's modulus convention)."""
    if isinstance(m, sp.Pow) and m.exp == 2:
        return m.base
    return sp.sqrt(m)


def revArgs(x, name):
    """Arguments in Maple order for a SymPy function application."""
    a = list(x.args)
    if isinstance(x, sp.polygamma):
        if a[0] == 0:
            return 'Psi', [a[1]]
        return 'Psi', a
    if isinstance(x, sp.digamma):
        return 'Psi', a
    if isinstance(x, sp.trigamma):
        return 'Psi', [sp.Integer(1), a[0]]
    if isinstance(x, sp.expint):
        return 'Ei', a
    if isinstance(x, sp.hyper):
        return 'hypergeom', [MList(list(x.ap)), MList(list(x.bq)), a[2]]
    if isinstance(x, sp.LambertW):
        return 'LambertW', ([a[0]] if len(a) == 1 or a[1] == 0 else [a[1], a[0]])
    if isinstance(x, sp.elliptic_k):
        return 'EllipticK', [unsq(a[0])]
    if isinstance(x, sp.elliptic_e):
        return 'EllipticE', ([unsq(a[0])] if len(a) == 1 else [a[0], unsq(a[1])])
    if isinstance(x, sp.elliptic_f):
        return 'EllipticF', [a[0], unsq(a[1])]
    if isinstance(x, sp.elliptic_pi):
        return 'EllipticPi', (a[:-1] + [unsq(a[-1])])
    if isinstance(x, sp.Heaviside):
        return 'Heaviside', a[:1]
    if isinstance(x, sp.airyaiprime):
        return 'AiryAi', [sp.Integer(1), a[0]]
    if isinstance(x, sp.airybiprime):
        return 'AiryBi', [sp.Integer(1), a[0]]
    if isinstance(x, sp.zeta) and len(a) == 2:
        return 'Zeta', a
    if isinstance(x, (sp.LaplaceTransform, sp.InverseLaplaceTransform, sp.FourierTransform,
                      sp.InverseFourierTransform, sp.MellinTransform)):
        return name, a
    return name, a


def exactFloats(e):
    """Replace every Float by the exact decimal it displays, so tiny binary noise cannot fool a test."""
    fl = list(e.atoms(sp.Float))
    if not fl:
        return e, None
    rep = {}
    nd = 10 ** 6
    for f in fl:
        n = max(1, prec_to_dps(f._prec))
        nd = min(nd, n)
        rep[f] = sp.Rational(to_str(f._mpf_, n, strip_zeros=True, min_fixed=-10 ** 6, max_fixed=10 ** 6))
    return e.xreplace(rep), nd


class Convert(object):
    """EXPR (JSON tree) <-> SymPy / engine objects, honouring assumptions and Digits."""

    def __init__(self, assume, digits):
        self.assume = assume
        self.digits = digits
        self.printDigits = digits
        self.floatDigits = None
        self.keepZeros = False
        self.engine = None
        self.nodes = {
            'int': self.nInt, 'rat': self.nRat, 'float': self.nFloat, 'sym': self.nSym,
            'idx': self.nIdx, 'str': self.nStr, 'bool': self.nBool, 'fail': self.nFail,
            'const': self.nConst, 'add': self.nAdd, 'mul': self.nMul, 'pow': self.nPow,
            'fn': self.nFn, 'fnidx': self.nFnIdx, 'eq': self.nRel, 'neq': self.nRel, 'lt': self.nRel,
            'le': self.nRel, 'and': self.nAnd, 'or': self.nOr, 'not': self.nNot,
            'list': self.nList, 'set': self.nSet, 'seq': self.nSeq, 'range': self.nRange,
            'matrix': self.nMatrix, 'vector': self.nVector, 'proc': self.nProc, 'apply': self.nApply,
        }

    # ---- EXPR -> objects ----

    def toSympy(self, e):
        if isinstance(e, dict):
            for k, f in self.nodes.items():
                if k in e:
                    return f(e)
            raise MapleError('invalid expression')
        if isinstance(e, bool):
            return sp.true if e else sp.false
        if isinstance(e, int):
            return sp.Integer(e)
        raise MapleError('invalid expression')

    def flat(self, items):
        out = []
        for x in items:
            if isinstance(x, Seq):
                out.extend(x)
            else:
                out.append(x)
        return out

    def sym(self, name):
        if self.assume is None:
            return sp.Symbol(name)
        return self.assume.symbol(name)

    def nInt(self, e):
        return sp.Integer(int(e['int']))

    def nRat(self, e):
        return sp.Rational(int(e['rat'][0]), int(e['rat'][1]))

    def nFloat(self, e):
        s = str(e['float'])
        nd = len(re.sub(r'[^0-9]', '', s.lower().split('e')[0]).lstrip('0')) or 1
        d = max(int(e.get('digits') or nd), nd, self.digits)
        self.printDigits = max(self.printDigits, d)
        try:
            return sp.Float(s, d)
        except (ValueError, TypeError):
            raise MapleError('invalid floating-point number')

    def nSym(self, e):
        return self.sym(e['sym'])

    def nIdx(self, e):
        base, idx = e['idx']
        parts = [self.idxText(i) for i in idx]
        name = '{}[{}]'.format(self.idxText(base), ','.join(parts))
        s = IdxSym(name)
        s._idxBase = base
        s._idxArgs = idx
        return s

    def idxText(self, node):
        if isinstance(node, dict):
            if 'sym' in node:
                return node['sym']
            if 'int' in node:
                return str(node['int'])
        return json.dumps(node, sort_keys=True, separators=(',', ':'))

    def nStr(self, e):
        return Str(e['str'])

    def nBool(self, e):
        return sp.true if e['bool'] else sp.false

    def nFail(self, e):
        return FAIL

    def nConst(self, e):
        c = e['const']
        table = {'Pi': sp.pi, 'I': sp.I, 'infinity': sp.oo, 'gamma': sp.EulerGamma,
                 'Catalan': sp.Catalan, 'undefined': sp.nan}
        if c not in table:
            raise MapleError('unknown constant {}'.format(c))
        return table[c]

    def nAdd(self, e):
        return opAdd(self.flat([self.toSympy(x) for x in e['add']]))

    def nMul(self, e):
        return opMul(self.flat([self.toSympy(x) for x in e['mul']]))

    def nPow(self, e):
        b, x = e['pow']
        return opPow(self.toSympy(b), self.toSympy(x))

    def nFn(self, e):
        name = e['fn']
        args = self.flat([self.toSympy(x) for x in e.get('args', [])])
        if isinstance(name, dict):
            return self.applyExpr(name, args)
        return self.applyFn(name, args)

    def nApply(self, e):
        f, args = e['apply']
        return self.applyExpr(f, self.flat([self.toSympy(x) for x in args]))

    def applyFn(self, name, a):
        short = shortName(name)
        if short in FUNCS:
            try:
                return floatContagion(FUNCS[short](*a))
            except MapleError:
                raise
            except TypeError:
                raise MapleError('(in {}) wrong number (or type) of arguments'.format(short))
        if short in INERT:
            return INERT[short](*a)
        if short in NESTED and self.engine is not None:
            return self.engine.nested(short, a)
        if short == 'RootOf':
            return rootOf(a)
        if short in ('Matrix', 'Vector') and self.engine is not None:
            return self.engine.nested(short, a)
        return sp.Function(name)(*[x if isinstance(x, sp.Basic) else Wrap(x) for x in a])

    def applyExpr(self, fexpr, a):
        """Application of a computed function, e.g. D(y)(0) or (x -> x^2)(3)."""
        if isinstance(fexpr, dict) and 'proc' in fexpr:
            lam = self.nProc(fexpr)
            return lam(*a)
        if isinstance(fexpr, dict) and 'sym' in fexpr:
            return sp.Function(fexpr['sym'])(*a)
        order, target = self.dOperator(fexpr)
        if target is not None:
            return dApply(target, order, a)
        raise MapleError('invalid function application')

    def dOperator(self, node):
        """Parse D(f), D[1](f), D[1,1](f) or (D@@n)(f); returns (order, f object)."""
        if not isinstance(node, dict):
            return None, None
        if 'fn' in node and node['fn'] == 'D' and len(node.get('args', [])) == 1:
            return 1, self.toSympy(node['args'][0])
        if 'fnidx' in node and node['fnidx'][0] == 'D' and len(node['fnidx'][2]) == 1:
            idx = node['fnidx'][1]
            return len(idx), self.toSympy(node['fnidx'][2][0])
        if 'fn' in node and isinstance(node['fn'], dict):
            inner = node['fn']
            if 'fn' in inner and inner['fn'] == '@@' and len(inner.get('args', [])) == 2:
                try:
                    n = int(inner['args'][1]['int'])
                except (KeyError, TypeError, ValueError):
                    return None, None
                if len(node.get('args', [])) == 1:
                    return n, self.toSympy(node['args'][0])
        return None, None

    def nFnIdx(self, e):
        name, idx, args = e['fnidx']
        a = self.flat([self.toSympy(x) for x in args])
        if name == 'evalf' and len(idx) == 1:
            n = intOf(self.toSympy(idx[0]))
            self.printDigits = max(self.printDigits, n)
            return a[0].evalf(n) if isinstance(a[0], sp.Basic) else a[0]
        if name == 'log' and len(idx) == 1 and len(a) == 1:
            return lnFn(a[0], self.toSympy(idx[0]))
        if name == 'D':
            return dIndexed([self.toSympy(i) for i in idx], a)
        if name in ('Vector', 'Matrix') and self.engine is not None:
            return self.engine.nested(name, a, [self.toSympy(i) for i in idx])
        if name in PACKAGES and idx and isinstance(idx[0], dict) and 'sym' in idx[0]:
            return self.applyFn(idx[0]['sym'], a)
        enc = '{}\x00{}'.format(name, json.dumps(idx, sort_keys=True, separators=(',', ':')))
        return sp.Function(enc)(*a)

    def nRel(self, e):
        op = [k for k in ('eq', 'neq', 'lt', 'le') if k in e][0]
        a, b = e[op]
        return mkRel(op, self.toSympy(a), self.toSympy(b))

    def nAnd(self, e):
        items = [self.toSympy(x) for x in e['and']]
        try:
            return sp.And(*items)
        except (TypeError, ValueError):
            raise MapleError('invalid boolean expression')

    def nOr(self, e):
        items = [self.toSympy(x) for x in e['or']]
        try:
            return sp.Or(*items)
        except (TypeError, ValueError):
            raise MapleError('invalid boolean expression')

    def nNot(self, e):
        x = self.toSympy(e['not'])
        try:
            return sp.Not(x)
        except (TypeError, ValueError):
            raise MapleError('invalid boolean expression')

    def nList(self, e):
        return MList(self.flat([self.toSympy(x) for x in e['list']]))

    def nSet(self, e):
        return mkSet(self.flat([self.toSympy(x) for x in e['set']]))

    def nSeq(self, e):
        return Seq(self.flat([self.toSympy(x) for x in e['seq']]))

    def nRange(self, e):
        return Rng(self.toSympy(e['range'][0]), self.toSympy(e['range'][1]))

    def nMatrix(self, e):
        rows = [[self.toSympy(x) for x in row] for row in e['matrix']]
        if not rows:
            return Mat(sp.zeros(0, 0))
        return Mat(sp.Matrix(rows))

    def nVector(self, e):
        items = [self.toSympy(x) for x in e['vector']]
        orient = e.get('orient', 'column')
        if orient == 'row':
            return Mat(sp.Matrix([items]) if items else sp.zeros(1, 0), 'row')
        return Mat(sp.Matrix(items) if items else sp.zeros(0, 1), 'col')

    def nProc(self, e):
        params = [self.sym(p) for p in e['proc']['params']]
        body = self.toSympy(e['proc']['body'])
        return sp.Lambda(tuple(params), body)

    # ---- objects -> EXPR ----

    def toExpr(self, x):
        if isinstance(x, Raw):
            return x.node
        if x is True or x is False:
            return {'bool': bool(x)}
        if x is FAIL:
            return {'fail': True}
        if isinstance(x, int):
            return {'int': str(x)}
        if isinstance(x, Str):
            return {'str': str(x)}
        if isinstance(x, str):
            return {'str': x}
        if isinstance(x, Seq):
            return {'seq': [self.toExpr(i) for i in x]}
        if isinstance(x, MList):
            return {'list': [self.toExpr(i) for i in x]}
        if isinstance(x, MSet):
            return {'set': [self.toExpr(i) for i in sortSet(x)]}
        if isinstance(x, (list, tuple)):
            return {'list': [self.toExpr(i) for i in x]}
        if isinstance(x, (set, frozenset)):
            return {'set': [self.toExpr(i) for i in sortSet(list(x))]}
        if isinstance(x, Rng):
            return {'range': [self.toExpr(x.lo), self.toExpr(x.hi)]}
        if isinstance(x, Mat):
            return self.matNode(x)
        if isinstance(x, RelObj):
            return {x.op: [self.toExpr(x.lhs), self.toExpr(x.rhs)]}
        if isinstance(x, sp.MatrixBase):
            return self.matNode(Mat(x))
        if isinstance(x, sp.Basic):
            return self.fromBasic(x)
        raise MapleError('unable to convert result {}'.format(type(x).__name__))

    def matNode(self, m):
        mm = m.m
        if m.kind == 'col':
            return {'vector': [self.toExpr(mm[i, 0]) for i in range(mm.rows)], 'orient': 'column'}
        if m.kind == 'row':
            return {'vector': [self.toExpr(mm[0, j]) for j in range(mm.cols)], 'orient': 'row'}
        return {'matrix': [[self.toExpr(mm[i, j]) for j in range(mm.cols)] for i in range(mm.rows)]}

    def intNode(self, i):
        return {'int': str(int(i))}

    def fromBasic(self, x):
        if isinstance(x, sp.Integer):
            return self.intNode(x)
        if isinstance(x, sp.Rational):
            return {'rat': [str(x.p), str(x.q)]}
        if isinstance(x, sp.Float):
            return self.floatNode(x)
        if isinstance(x, Wrap):
            return self.toExpr(x.obj)
        if isinstance(x, IdxSym):
            return {'idx': [x._idxBase, x._idxArgs]}
        if isinstance(x, sp.Symbol):
            name = x.name
            if isinstance(x, sp.Dummy):
                name = '_{}{}'.format(name, x.dummy_index % 1000 if hasattr(x, 'dummy_index') else '')
            return {'sym': name}
        if x is sp.pi:
            return {'const': 'Pi'}
        if x is sp.I:
            return {'const': 'I'}
        if x is sp.oo:
            return {'const': 'infinity'}
        if x is sp.S.NegativeInfinity:
            return {'mul': [self.intNode(-1), {'const': 'infinity'}]}
        if x is sp.nan or x is sp.zoo:
            return {'const': 'undefined'}
        if x is sp.EulerGamma:
            return {'const': 'gamma'}
        if x is sp.Catalan:
            return {'const': 'Catalan'}
        if x is sp.E:
            return {'fn': 'exp', 'args': [self.intNode(1)]}
        if x is sp.true:
            return {'bool': True}
        if x is sp.false:
            return {'bool': False}
        if isinstance(x, sp.Add):
            return {'add': [self.toExpr(t) for t in orderedTerms(x)]}
        if isinstance(x, sp.Mul):
            return {'mul': [self.toExpr(t) for t in orderedFactors(x)]}
        if isinstance(x, sp.Pow):
            return {'pow': [self.toExpr(x.base), self.toExpr(x.exp)]}
        if isinstance(x, sp.core.relational.Relational):
            op, l, r = relParts(x)
            return {op: [self.toExpr(l), self.toExpr(r)]}
        if isinstance(x, sp.And):
            return {'and': [self.toExpr(t) for t in x.args]}
        if isinstance(x, sp.Or):
            return {'or': [self.toExpr(t) for t in x.args]}
        if isinstance(x, sp.Not):
            return {'not': self.toExpr(x.args[0])}
        if isinstance(x, sp.Piecewise):
            out = []
            for e, c in x.args:
                if c is sp.true and len(out) >= 0 and (e, c) == x.args[-1]:
                    out.append(self.toExpr(e))
                else:
                    out.append(self.toExpr(c))
                    out.append(self.toExpr(e))
            return {'fn': 'piecewise', 'args': out}
        if isinstance(x, sp.Derivative):
            args = [self.toExpr(x.expr)]
            for v, n in x.variable_count:
                args.extend([self.toExpr(v)] * int(n))
            inert = not x.expr.has(sp.core.function.AppliedUndef)
            return {'fn': 'Diff' if inert else 'diff', 'args': args}
        if isinstance(x, sp.Subs):
            return self.subsNode(x)
        if isinstance(x, sp.Integral):
            return {'fn': 'Int', 'args': [self.toExpr(x.function)] + [self.limitSpec(l) for l in x.limits]}
        if isinstance(x, sp.Sum):
            return {'fn': 'Sum', 'args': [self.toExpr(x.function)] + [self.limitSpec(l) for l in x.limits]}
        if isinstance(x, sp.Product):
            return {'fn': 'Product', 'args': [self.toExpr(x.function)] + [self.limitSpec(l) for l in x.limits]}
        if isinstance(x, sp.Limit):
            f, v, p = x.args[:3]
            args = [self.toExpr(f), {'eq': [self.toExpr(v), self.toExpr(p)]}]
            if len(x.args) > 3 and str(x.args[3]) in ('+', '-'):
                args.append({'sym': 'right' if str(x.args[3]) == '+' else 'left'})
            return {'fn': 'Limit', 'args': args}
        if isinstance(x, sp.Order):
            return {'fn': 'O', 'args': [self.toExpr(x.expr)]}
        if isinstance(x, sp.Lambda):
            return self.lambdaNode(x)
        if isinstance(x, sp.CRootOf):
            return self.rootOfNode(x)
        if isinstance(x, sp.AccumBounds):
            return {'range': [self.toExpr(x.min), self.toExpr(x.max)]}
        if isinstance(x, sp.Interval):
            return {'range': [self.toExpr(x.start), self.toExpr(x.end)]}
        if isinstance(x, sp.FiniteSet):
            return {'set': [self.toExpr(i) for i in sortSet(list(x.args))]}
        if isinstance(x, sp.Tuple):
            return {'list': [self.toExpr(i) for i in x.args]}
        if isinstance(x, sp.Indexed):
            return {'idx': [self.toExpr(x.base.label), [self.toExpr(i) for i in x.indices]]}
        if isinstance(x, sp.Function) or hasattr(x, 'func') and isinstance(x, sp.core.function.Application):
            return self.fnNode(x)
        raise MapleError('unable to convert result {}'.format(type(x).__name__))

    def limitSpec(self, lim):
        if len(lim) == 3:
            return {'eq': [self.toExpr(lim[0]), {'range': [self.toExpr(lim[1]), self.toExpr(lim[2])]}]}
        return self.toExpr(lim[0])

    def lambdaNode(self, x):
        params = []
        for p in x.variables:
            params.append(p.name)
        return {'proc': {'params': params, 'body': self.toExpr(x.expr)}}

    def rootOfNode(self, x):
        z = sp.Symbol('_Z')
        poly = x.poly.as_expr().subs(x.poly.gen, z)
        return {'fn': 'RootOf', 'args': [self.toExpr(poly), {'eq': [{'sym': 'index'}, self.intNode(x.index + 1)]}]}

    def subsNode(self, x):
        expr, vs, pts = x.args
        if len(vs) == 1 and isinstance(expr, sp.Derivative):
            f = expr.expr
            if isinstance(f, sp.core.function.AppliedUndef) and len(f.args) == 1 and f.args[0] == vs[0] \
                    and len(expr.variable_count) == 1 and expr.variable_count[0][0] == vs[0]:
                n = int(expr.variable_count[0][1])
                fsym = {'sym': f.func.__name__}
                if n == 1:
                    head = {'fn': 'D', 'args': [fsym]}
                else:
                    head = {'fnidx': ['D', [self.intNode(1)] * n, [fsym]]}
                return {'fn': head, 'args': [self.toExpr(pts[0])]}
        args = [self.toExpr(expr)]
        for v, p in zip(vs, pts):
            args.append({'eq': [self.toExpr(v), self.toExpr(p)]})
        return {'fn': 'eval', 'args': args}

    def fnNode(self, x):
        cls = type(x)
        if cls in REV:
            name, args = revArgs(x, REV[cls])
        elif isinstance(x, (sp.digamma, sp.trigamma, sp.polygamma, sp.hyper, sp.LambertW, sp.expint,
                            sp.airyaiprime, sp.airybiprime, sp.elliptic_k, sp.elliptic_e,
                            sp.elliptic_f, sp.elliptic_pi)):
            name, args = revArgs(x, None)
        elif isinstance(x, sp.log):
            name, args = 'ln', list(x.args)
        else:
            name = cls.__name__
            args = list(x.args)
            if '\x00' in name:
                base, js = name.split('\x00', 1)
                return {'fnidx': [base, json.loads(js), [self.toExpr(a) for a in args]]}
        return {'fn': name, 'args': [self.toExpr(a) for a in args]}

    def floatNode(self, f):
        n = self.floatDigits or max(1, min(self.printDigits, prec_to_dps(f._prec)))
        if f.is_zero:
            return {'float': '0.', 'digits': max(self.digits, 1)}
        s = to_str(f._mpf_, n, strip_zeros=False, min_fixed=-6, max_fixed=n)
        mant, _, ex = s.partition('e')
        if not self.keepZeros or self.isIntegral(f):
            if '.' in mant:
                mant = mant.rstrip('0')
        if ex:
            ex = str(int(ex))
            if mant.endswith('.'):
                mant += '0'
            s = '{}e{}'.format(mant, ex)
        else:
            s = mant
        nsig = len(re.sub(r'[^0-9]', '', mant).lstrip('0')) or 1
        return {'float': s, 'digits': max(self.digits, nsig)}

    def isIntegral(self, f):
        try:
            v = mpmath.mpf(f._mpf_)
            return v == mpmath.floor(v) and abs(v) < 10 ** 15
        except (TypeError, ValueError):
            return False


HANDLERS = {}
INERT = {}
NESTED = {'IdentityMatrix', 'ZeroMatrix', 'DiagonalMatrix', 'Transpose', 'HermitianTranspose', 'MatrixInverse',
          'Multiply', 'MatrixMatrixMultiply', 'MatrixVectorMultiply', '.', 'MatrixPower', 'MatrixExponential',
          'CrossProduct', 'Normalize', 'Determinant', 'Trace', 'Rank', 'DotProduct', 'Norm', 'Dimension'}


def sortSet(items):
    def key(x):
        try:
            if isinstance(x, sp.Basic):
                return (0, sp.default_sort_key(x))
            return (1, str(x))
        except (TypeError, ValueError):
            return (2, str(x))
    try:
        return sorted(items, key=key)
    except TypeError:
        return list(items)


def orderedTerms(x):
    try:
        return x.as_ordered_terms()
    except (TypeError, ValueError, NotImplementedError, AttributeError):
        return list(x.args)


def orderedFactors(x):
    try:
        coeff, rest = x.as_coeff_Mul()
        out = [coeff] if coeff != 1 else []
        if isinstance(rest, sp.Mul):
            out.extend(rest.as_ordered_factors())
        elif rest != 1:
            out.append(rest)
        return out
    except (TypeError, ValueError, NotImplementedError, AttributeError):
        return list(x.args)


def rootOf(a):
    if not a:
        raise MapleError('wrong number (or type) of arguments')
    expr = a[0]
    index = None
    var = None
    for extra in a[1:]:
        if isRel(extra):
            op, l, r = relParts(extra)
            if isSym(l) and l.name == 'index':
                index = r
        elif isSym(extra):
            var = extra
    z = var if var is not None else next((s for s in expr.free_symbols if s.name == '_Z'), None)
    if z is None:
        fs = list(expr.free_symbols)
        z = fs[0] if len(fs) == 1 else sp.Symbol('_Z')
    if index is not None and isinstance(index, sp.Integer):
        try:
            poly = sp.Poly(expr, z)
            if poly.degree() >= 1:
                return sp.CRootOf(poly, int(index) - 1)
        except (sp.PolynomialError, IndexError, NotImplementedError):
            pass
    args = [expr] + [x for x in a[1:]]
    return sp.Function('RootOf')(*args)


def dApply(target, order, a):
    """D(f)(a...) evaluated at the point a (order-th derivative)."""
    if len(a) != 1:
        raise MapleError('invalid arguments to D')
    pt = a[0]
    if isinstance(target, sp.Lambda):
        v = target.variables[0]
        return sp.diff(target.expr, (v, order)).subs(v, pt)
    if isSym(target):
        name = target.name
        if name in FUNCS:
            t = sp.Dummy('t')
            return sp.diff(FUNCS[name](t), (t, order)).subs(t, pt)
        t = sp.Dummy('t')
        f = sp.Function(name)
        if isSym(pt):
            return sp.Derivative(f(pt), (pt, order))
        return sp.Subs(sp.Derivative(f(t), (t, order)), t, pt)
    raise MapleError('invalid arguments to D')


def dIndexed(idx, a):
    """D[i,j](f): partial derivative of a procedure with respect to its i-th, j-th parameter."""
    if len(a) != 1 or not isinstance(a[0], sp.Lambda):
        return sp.Function('D')(*a)
    lam = a[0]
    body = lam.expr
    for i in idx:
        k = intOf(i)
        body = sp.diff(body, lam.variables[k - 1])
    return sp.Lambda(lam.variables, body)


# ----------------------------------------------------------------------------
# Arithmetic on Maple structures (matrices behave like Maple, scalars like SymPy)
# ----------------------------------------------------------------------------

def opAdd(items):
    mats = [x for x in items if isinstance(x, Mat)]
    if not mats:
        for x in items:
            if not isinstance(x, sp.Basic):
                raise MapleError('wrong number (or type) of arguments')
        return floatContagion(sp.Add(*items))
    acc = None
    for x in items:
        if isinstance(x, Mat):
            if acc is None:
                acc = Mat(x.m, x.kind)
            else:
                if acc.m.shape != x.m.shape:
                    raise MapleError('(in rtable/Sum) invalid arguments: dimensions do not match')
                acc = Mat(acc.m + x.m, acc.kind)
    for x in items:
        if not isinstance(x, Mat):
            if acc.kind != 'matrix' or acc.m.rows != acc.m.cols:
                raise MapleError('(in rtable/Sum) invalid arguments')
            acc = Mat(acc.m + x * sp.eye(acc.m.rows), acc.kind)
    return acc


def opMul(items):
    if not any(isinstance(x, Mat) for x in items):
        for x in items:
            if not isinstance(x, sp.Basic):
                raise MapleError('wrong number (or type) of arguments')
        return floatContagion(sp.Mul(*items))
    scalar = sp.Integer(1)
    mats = []
    for x in items:
        if isinstance(x, Mat):
            mats.append(x)
        else:
            scalar = scalar * x
    acc = mats[0]
    for m in mats[1:]:
        acc = matMul(acc, m)
    return Mat(acc.m * scalar, acc.kind)


def matMul(a, b):
    if a.m.cols != b.m.rows:
        raise MapleError('(in LinearAlgebra:-MatrixMatrixMultiply) first matrix column dimension (%d) <> second matrix row dimension (%d)' % (a.m.cols, b.m.rows))
    prod = a.m * b.m
    if a.kind == 'matrix' and b.kind == 'col':
        return Mat(prod, 'col')
    if a.kind == 'row' and b.kind == 'matrix':
        return Mat(prod, 'row')
    return Mat(prod, 'matrix')


def opPow(b, x):
    if isinstance(b, Mat):
        if not isinstance(x, sp.Basic) or not x.is_Integer:
            raise MapleError('(in LinearAlgebra:-MatrixPower) invalid exponent')
        if b.m.rows != b.m.cols:
            raise MapleError('(in LinearAlgebra:-MatrixPower) matrix must be square')
        return Mat(b.m ** int(x), b.kind)
    if not isinstance(b, sp.Basic) or not isinstance(x, sp.Basic):
        raise MapleError('wrong number (or type) of arguments')
    if b == 0 and x.is_number and (x.is_negative or x.is_real is False):
        raise MapleError('numeric exception: division by zero')
    return floatContagion(sp.Pow(b, x))


# ----------------------------------------------------------------------------
# Printers with Maple flavour (ln, e, arcsin, GAMMA ...)
# ----------------------------------------------------------------------------

class MapleLatex(LatexPrinter):
    def _print_log(self, expr, exp=None):
        arg = self._print(expr.args[0])
        if exp is not None:
            return r'\ln^{%s}{\left(%s \right)}' % (exp, arg)
        return r'\ln{\left(%s \right)}' % arg

    def _print_Csgn(self, expr, exp=None):
        return r'\operatorname{csgn}{\left(%s \right)}' % self._print(expr.args[0])

    def _print_MList(self, x):
        return r'\left[ ' + ', '.join(self._print(i) for i in x) + r' \right]'

    def _print_MSet(self, x):
        return r'\left\{ ' + ', '.join(self._print(i) for i in sortSet(x)) + r' \right\}'

    def _print_Seq(self, x):
        return ', '.join(self._print(i) for i in x)

    def _print_Mat(self, x):
        return self._print(x.m)

    def _print_Rng(self, x):
        return '%s \\dots %s' % (self._print(x.lo), self._print(x.hi))

    def _print_Str(self, x):
        return r'\mathrm{%s}' % str(x)

    def _print_Fail(self, x):
        return r'\mathrm{FAIL}'

    def _print_RelObj(self, x):
        sym = {'eq': '=', 'neq': r'\neq', 'lt': '<', 'le': r'\leq'}[x.op]
        return '%s %s %s' % (self._print(x.lhs), sym, self._print(x.rhs))

    def _print_Lambda(self, x):
        vs = ', '.join(self._print(v) for v in x.variables)
        return r'%s \mapsto %s' % (vs, self._print(x.expr))


class MaplePretty(PrettyPrinter):
    def _print_log(self, e):
        return self._print_Function(e, func_name='ln')

    def _print_Csgn(self, e):
        return self._print_Function(e, func_name='csgn')

    def _print_Exp1(self, e):
        return self._print_Symbol(sp.Symbol('e'))

    def _print_ExpBase(self, e):
        from sympy.printing.pretty.stringpict import prettyForm
        return prettyForm('e') ** self._print(e.args[0])

    def _print_ImaginaryUnit(self, e):
        return self._print_Symbol(sp.Symbol('I'))

    def _print_MSet(self, x):
        return self._print_seq(sortSet(x), '{', '}')

    def _print_Seq(self, x):
        return self._print_seq(list(x), '', '')

    def _print_Mat(self, x):
        return self._print(x.m)

    def _print_Rng(self, x):
        from sympy.printing.pretty.stringpict import prettyForm
        return prettyForm(*self._print(x.lo).right(' .. ', self._print(x.hi)))

    def _print_Str(self, x):
        from sympy.printing.pretty.stringpict import prettyForm
        return prettyForm('"%s"' % str(x))

    def _print_Fail(self, x):
        from sympy.printing.pretty.stringpict import prettyForm
        return prettyForm('FAIL')

    def _print_RelObj(self, x):
        from sympy.printing.pretty.stringpict import prettyForm
        sym = {'eq': ' = ', 'neq': ' <> ', 'lt': ' < ', 'le': ' <= '}[x.op]
        return prettyForm(*self._print(x.lhs).right(sym, self._print(x.rhs)))

    def _print_Lambda(self, x):
        from sympy.printing.pretty.stringpict import prettyForm
        vs = ', '.join(v.name for v in x.variables)
        return prettyForm(*prettyForm(vs + ' -> ').right(self._print(x.expr)))


# ----------------------------------------------------------------------------
# Shared helpers for the handler families
# ----------------------------------------------------------------------------

def optName(x):
    if isSym(x):
        return x.name
    if isinstance(x, sp.core.function.AppliedUndef):
        return x.func.__name__
    if isinstance(x, Str):
        return str(x)
    return None


def needArgs(a, lo, hi=None):
    n = len(a)
    if n < lo or (hi is not None and n > hi):
        raise MapleError('wrong number (or type) of arguments')


def freeNames(e):
    return sorted(s.name for s in e.free_symbols if isSym(s))


def theVar(e, given=None):
    """Variable to use: the given one, else the only free symbol of e."""
    if given is not None:
        return given
    fs = sorted([s for s in e.free_symbols if isSym(s)], key=lambda s: s.name)
    if len(fs) == 1:
        return fs[0]
    if not fs:
        return sp.Symbol('x')
    raise MapleError('wrong number (or type) of arguments')


def splitSpec(spec):
    """x or x=a..b or x=a  ->  (x, a, b) ; a, b None when absent."""
    if isRel(spec):
        op, l, r = relParts(spec)
        if isinstance(r, Rng):
            return l, r.lo, r.hi
        return l, r, None
    return spec, None, None


def expandLogNumbers(e):
    """ln(c*x) -> ln(c)+ln(x) for positive numeric c, as Maple's expand does."""
    def rep(x):
        coeff, rest = x.args[0].as_coeff_Mul()
        if coeff.is_positive and coeff != 1 and rest != 1:
            return sp.log(coeff) + sp.log(rest)
        return x
    return e.replace(lambda x: isinstance(x, sp.log) and isinstance(x.args[0], sp.Mul), rep)


class ArithMixin(object):
    """simplify, expand, factor, normal, combine, collect, evalf, convert ... (Maple names)."""

    def assumedMap(self, e, props, fn):
        """Run fn(e) with every free symbol carrying extra assumptions, mapping back to plain names."""
        fwd = {}
        for s in e.free_symbols:
            if isSym(s) and not isinstance(s, (sp.Dummy, IdxSym)):
                merged = dict(self.assume.props.get(s.name, {}))
                merged.update(props)
                try:
                    fwd[s] = sp.Symbol(s.name, **merged)
                except (ValueError, TypeError):
                    continue
        back = {v: k for k, v in fwd.items()}
        res = fn(e.xreplace(fwd))
        if isinstance(res, sp.Basic):
            res = res.xreplace(back)
        return res

    def one(self, x, fn):
        return fn(x) if isinstance(x, sp.Basic) else x

    def doSimplify(self, a):
        needArgs(a, 1)
        names = []
        assumeProp = None
        for o in a[1:]:
            n = optName(o)
            if n:
                names.append(n)
            elif isRel(o):
                op, l, r = relParts(o)
                if isSym(l) and l.name == 'assume' and optName(r) in TYPE_PROPS:
                    assumeProp = TYPE_PROPS[optName(r)]

        def simp(x):
            if 'trig' in names:
                return sp.trigsimp(x)
            if 'radical' in names or 'sqrt' in names:
                return sp.radsimp(sp.simplify(x))
            if 'ln' in names:
                return sp.simplify(sp.logcombine(x))
            if 'exp' in names:
                return sp.simplify(sp.powsimp(x))
            if 'hypergeom' in names:
                return sp.hyperexpand(x)
            best = sp.simplify(x)
            if x.has(sp.sin, sp.cos, sp.tan, sp.cot, sp.sec, sp.csc):
                alt = sp.trigsimp(sp.expand(x))
                if alt.count_ops() < best.count_ops():
                    best = alt
            if best != 0 and any(f.args[0].has(sp.I) for f in best.atoms(sp.exp)):
                # Euler: exp(I*y) -> cos(y) + I*sin(y), kept only when it is simpler
                alt = sp.simplify(best.replace(lambda f: isinstance(f, sp.exp) and f.args[0].has(sp.I),
                                               lambda f: f.rewrite(sp.cos)))
                if alt.count_ops() < best.count_ops():
                    best = alt
            return best

        def go(x):
            if not isinstance(x, sp.Basic):
                return x
            if assumeProp:
                return self.assumedMap(x, assumeProp, simp)
            return simp(x)
        return mapS(go, a[0])

    def doExpand(self, a):
        needArgs(a, 1)

        def go(x):
            r = sp.expand(x, trig=True)
            return expandLogNumbers(r)
        return mapS(lambda x: self.one(x, go), a[0])

    def doFactor(self, a):
        needArgs(a, 1)
        exts = []
        for o in a[1:]:
            if isinstance(o, sp.Basic) and not isSym(o):
                exts.append(o)

        def go(x):
            if x.is_Integer or x.is_Rational:
                return x
            if exts:
                return sp.factor(x, extension=exts if len(exts) > 1 else exts[0])
            return sp.factor(x)
        return mapS(lambda x: self.one(x, go), a[0])

    def doNormal(self, a):
        needArgs(a, 1)

        def go(x):
            r = sp.cancel(sp.together(x))
            n, d = sp.fraction(r)
            if d != 1:
                return sp.expand(n) / sp.factor(d)
            return r
        return mapS(lambda x: self.one(x, go), a[0])

    def doCombine(self, a):
        needArgs(a, 1)
        opts = [optName(o) for o in a[1:]]

        def go(x):
            r = x
            allOpts = not opts
            if allOpts or 'power' in opts or 'exp' in opts:
                r = sp.powsimp(r)
            if allOpts or 'ln' in opts:
                r = sp.logcombine(r)
            if (allOpts and r.has(sp.sin, sp.cos, sp.tan)) or 'trig' in opts:
                r = combineTrig(r)
            if 'radical' in opts:
                r = sp.radsimp(r)
            return r
        return mapS(lambda x: self.one(x, go), a[0])

    def doCollect(self, a):
        needArgs(a, 2)
        e, x = a[0], a[1]
        post = optName(a[2]) if len(a) > 2 else None
        xs = list(x) if isinstance(x, (MList, MSet)) else [x]

        def go(ex):
            ex = sp.expand(ex)
            if post in ('factor', 'simplify', 'normal', 'expand'):
                parts = sp.collect(ex, xs, evaluate=False)
                fn = {'factor': sp.factor, 'simplify': sp.simplify, 'normal': sp.cancel, 'expand': sp.expand}[post]
                return sp.Add(*[fn(c) * k for k, c in parts.items()])
            r = sp.collect(ex, xs)
            if len(xs) == 1 and isSym(xs[0]) and isinstance(r, sp.Add):
                return self.collectedOrder(r, xs[0])
            return r
        return mapS(lambda ex: self.one(ex, go), e)

    def collectedOrder(self, r, x):
        """Terms of a collected sum by decreasing power of x, coefficient first (c*x^2+(a+b)*x+d)."""
        parts = []
        for t in sp.Add.make_args(r):
            c, rest = t.as_independent(x, as_Add=False)
            k = rest.as_coeff_exponent(x)[1] if rest != 1 else sp.Integer(0)
            parts.append((k, c, rest))
        parts.sort(key=lambda p: -float(p[0]) if p[0].is_number else 0)
        nodes = []
        for k, c, rest in parts:
            facs = ([self.cv.toExpr(c)] if c != 1 or rest == 1 else []) + ([self.cv.toExpr(rest)] if rest != 1 else [])
            nodes.append(facs[0] if len(facs) == 1 else {'mul': facs})
        return Raw({'add': nodes})

    def doSort(self, a):
        needArgs(a, 1)
        e = a[0]
        if isinstance(e, (MList, Seq)):
            items = list(e)
            if all(isNum(i) and i.is_real for i in items):
                items.sort(key=lambda i: float(i.evalf(30)))
            elif all(isinstance(i, Str) for i in items):
                items.sort()
            else:
                items = sortSet(items)
            return type(e)(items)
        return e

    def doRationalize(self, a):
        needArgs(a, 1)
        return mapS(lambda x: self.one(x, sp.radsimp), a[0])

    def doRadnormal(self, a):
        needArgs(a, 1)
        return mapS(lambda x: self.one(x, lambda y: sp.radsimp(sp.expand(y))), a[0])

    def doEvalc(self, a):
        needArgs(a, 1)

        def go(x):
            return self.assumedMap(x, {'real': True}, lambda y: sp.expand_complex(y))
        return mapS(lambda x: self.one(x, go), a[0])

    def doEvalf(self, a):
        needArgs(a, 1, 2)
        n = self.digits
        if len(a) == 2:
            n = intOf(a[1])
        elif self.callIdx:
            n = intOf(self.callIdx[0])
        self.cv.floatDigits = n
        self.cv.keepZeros = True

        def go(x):
            if x.has(sp.Limit):
                x = x.doit()
            return x.evalf(n + 5)
        return mapS(lambda x: self.one(x, go), a[0])

    def doEvalb(self, a):
        needArgs(a, 1)
        return sp.true if self.evalBool(a[0]) else sp.false

    def evalBool(self, r):
        if r is sp.true or r is True:
            return True
        if r is sp.false or r is False:
            return False
        if isinstance(r, sp.And):
            return all(self.evalBool(t) for t in r.args)
        if isinstance(r, sp.Or):
            return any(self.evalBool(t) for t in r.args)
        if isinstance(r, sp.Not):
            return not self.evalBool(r.args[0])
        if isRel(r):
            op, l, rr = relParts(r)
            if op in ('eq', 'neq'):
                eq = self.sameValue(l, rr)
                return eq if op == 'eq' else not eq
            if isNum(l) and isNum(rr) and l.is_real and rr.is_real:
                lv, rv = l.evalf(30), rr.evalf(30)
                return bool(lv < rv) if op == 'lt' else bool(lv <= rv)
            raise MapleError('cannot determine if this expression is true or false: {}'.format(self.brief(r)))
        raise MapleError('unable to evaluate expression to true or false')

    def brief(self, r):
        try:
            return sp.sstr(r)
        except (TypeError, ValueError):
            return 'relation'

    def sameValue(self, l, r):
        if isinstance(l, (MList, MSet, Seq, Mat)) or isinstance(r, (MList, MSet, Seq, Mat)):
            if type(l) is not type(r):
                return False
            if isinstance(l, Mat):
                return l.kind == r.kind and l.m == r.m
            if isinstance(l, MSet):
                return len(l) == len(r) and all(any(sameObj(i, j) for j in r) for i in l)
            return len(l) == len(r) and all(self.sameValue(i, j) for i, j in zip(l, r))
        if isNum(l) and isNum(r) and (l.has(sp.Float) or r.has(sp.Float)):
            lv, rv = l.evalf(self.digits + 5), r.evalf(self.digits + 5)
            return abs(lv - rv) <= 10 ** (-(self.digits - 1)) * max(1, abs(lv))
        return sameObj(l, r)

    def doIs(self, a):
        needArgs(a, 1, 2)
        e = a[0]
        if len(a) == 2:
            t = a[1]
            if optName(t) in ('positive', 'negative', 'nonneg', 'nonpositive', 'real', 'integer', 'rational',
                               'nonzero', 'complex', 'even', 'odd', 'prime', 'imaginary', 'posint',
                               'negint', 'nonnegint', 'zero', 'constant', 'numeric', 'RealRange'):
                return self.triBool(self.propOf(e, optName(t)))
            return self.triBool(self.typeCheck(e, t))
        if isRel(e):
            op, l, r = relParts(e)
            d = r - l
            if op == 'eq':
                z = sp.simplify(l - r)
                return self.triBool(True if z == 0 else (False if z.is_zero is False or (isNum(z) and z != 0) else None))
            if op == 'neq':
                z = sp.simplify(l - r)
                return self.triBool(False if z == 0 else (True if z.is_zero is False else None))
            if op == 'lt':
                return self.triBool(d.is_positive)
            return self.triBool(d.is_nonnegative)
        if isinstance(e, sp.Basic) and e in (sp.true, sp.false):
            return e
        return FAIL

    def triBool(self, v):
        if v is None:
            return FAIL
        return sp.true if v else sp.false

    def propOf(self, e, name):
        if not isinstance(e, sp.Basic):
            return False
        table = {
            'positive': e.is_positive, 'negative': e.is_negative, 'nonneg': e.is_nonnegative,
            'nonpositive': e.is_nonpositive, 'real': e.is_real, 'integer': e.is_integer,
            'rational': e.is_rational, 'nonzero': e.is_nonzero, 'complex': e.is_complex,
            'even': e.is_even, 'odd': e.is_odd, 'prime': e.is_prime, 'imaginary': e.is_imaginary,
            'zero': e.is_zero, 'constant': e.is_number or None, 'numeric': e.is_number or None,
        }
        if name == 'posint':
            return None if e.is_integer is None or e.is_positive is None else bool(e.is_integer and e.is_positive)
        if name == 'negint':
            return None if e.is_integer is None or e.is_negative is None else bool(e.is_integer and e.is_negative)
        if name == 'nonnegint':
            return None if e.is_integer is None or e.is_nonnegative is None else bool(e.is_integer and e.is_nonnegative)
        return table.get(name)

    def doTesteq(self, a):
        needArgs(a, 1, 2)
        if len(a) == 2:
            l, r = a
        elif isRel(a[0]):
            op, l, r = relParts(a[0])
        else:
            l, r = a[0], sp.Integer(0)
        res = self.verifyAny(l, r)
        if res['verdict'] == 'equal':
            return sp.true
        if res['verdict'] == 'not_equal':
            return sp.false
        return FAIL

    def doSubs(self, a):
        needArgs(a, 2)
        e = a[-1]
        eqs = self.cv.flat(a[:-1])
        return self.substitute(e, eqs, False)

    def doEval(self, a):
        needArgs(a, 1)
        e = a[0]
        eqs = []
        simultaneous = False
        for o in a[1:]:
            if isinstance(o, (MList, MSet)):
                eqs.extend(o)
                simultaneous = True
            else:
                eqs.append(o)
        return self.substitute(e, eqs, simultaneous)

    def substitute(self, e, eqs, simultaneous):
        pairs = []
        for q in eqs:
            if not (isRel(q) and relParts(q)[0] == 'eq'):
                raise MapleError('invalid substitution')
            op, l, r = relParts(q)
            pairs.append((l, r))

        def go(x):
            if not isinstance(x, sp.Basic):
                return x
            if simultaneous:
                return x.subs(pairs, simultaneous=True)
            for l, r in pairs:
                x = x.subs(l, r)
            return x
        return mapS(go, e)

    def doConvert(self, a):
        needArgs(a, 2)
        e = a[0]
        t = optName(a[1])
        if t is None and isinstance(a[1], sp.Basic) and hasattr(a[1], 'func'):
            t = a[1].func.__name__
        if t is None:
            raise MapleError('wrong number (or type) of arguments')
        meth = CONVERT.get(t)
        if meth is None:
            raise MapleError('unable to convert to {}'.format(t))
        return getattr(self, meth)(e, a[2:])

    # ---- convert targets ----

    def cvParfrac(self, e, rest):
        def go(x):
            var = theVar(x, rest[0] if rest else None)
            return sp.apart(x, var)
        return mapS(lambda x: self.one(x, go), e)

    def cvFullParfrac(self, e, rest):
        def go(x):
            var = theVar(x, rest[0] if rest else None)
            return sp.apart(x, var, full=True).doit()
        return mapS(lambda x: self.one(x, go), e)

    def cvExp(self, e, rest):
        return mapS(lambda x: self.one(x, lambda y: y.rewrite(sp.exp)), e)

    def cvTrig(self, e, rest):
        def go(y):
            r = y.rewrite(sp.cos) if y.has(sp.exp) else y
            return r.rewrite(sp.sin) if y.has(sp.sinh, sp.cosh, sp.tanh) else r
        return mapS(lambda x: self.one(x, go), e)

    def cvSincos(self, e, rest):
        def go(y):
            for cls, fn in ((sp.tan, lambda a: sp.sin(a) / sp.cos(a)), (sp.cot, lambda a: sp.cos(a) / sp.sin(a)),
                            (sp.sec, lambda a: 1 / sp.cos(a)), (sp.csc, lambda a: 1 / sp.sin(a)),
                            (sp.tanh, lambda a: sp.sinh(a) / sp.cosh(a)), (sp.coth, lambda a: sp.cosh(a) / sp.sinh(a)),
                            (sp.sech, lambda a: 1 / sp.cosh(a)), (sp.csch, lambda a: 1 / sp.sinh(a))):
                y = y.replace(lambda q, c=cls: isinstance(q, c), lambda q, f=fn: f(q.args[0]))
            return y
        return mapS(lambda x: self.one(x, go), e)

    def cvTan(self, e, rest):
        return mapS(lambda x: self.one(x, lambda y: y.rewrite(sp.tan)), e)

    def cvLn(self, e, rest):
        return mapS(lambda x: self.one(x, lambda y: y.rewrite(sp.log)), e)

    def cvPolynom(self, e, rest):
        def go(y):
            return y.removeO() if y.has(sp.Order) else y
        return mapS(lambda x: self.one(x, go), e)

    def cvRational(self, e, rest):
        return mapS(lambda x: self.one(x, lambda y: sp.nsimplify(y, rational=True)), e)

    def cvFloat(self, e, rest):
        return self.doEvalf([e])

    def cvListlike(self, e, rest):
        if isinstance(e, Mat):
            if e.kind == 'matrix':
                return MList([MList(list(e.m.row(i))) for i in range(e.m.rows)])
            return MList(list(e.m))
        if isinstance(e, MSet):
            return MList(sortSet(e))
        if isinstance(e, (MList, Seq)):
            return MList(list(e))
        return MList([e])

    def cvSetlike(self, e, rest):
        if isinstance(e, Mat):
            return mkSet(list(e.m))
        if isinstance(e, (MSet, MList, Seq)):
            return mkSet(list(e))
        return mkSet([e])

    def cvRadical(self, e, rest):
        return mapS(lambda x: self.one(x, lambda y: y.rewrite(sp.sqrt)), e)

    def cvPiecewise(self, e, rest):
        return mapS(lambda x: self.one(x, lambda y: y.rewrite(sp.Piecewise)), e)

    def cvHeaviside(self, e, rest):
        return mapS(lambda x: self.one(x, lambda y: y.rewrite(sp.Heaviside)), e)

    def cvBinomial(self, e, rest):
        return mapS(lambda x: self.one(x, lambda y: sp.combsimp(y.rewrite(sp.binomial))), e)

    def cvFactorial(self, e, rest):
        return mapS(lambda x: self.one(x, lambda y: y.rewrite(sp.factorial)), e)

    def cvGamma(self, e, rest):
        return mapS(lambda x: self.one(x, lambda y: y.rewrite(sp.gamma)), e)

    def cvHorner(self, e, rest):
        def go(y):
            var = theVar(y, rest[0] if rest else None)
            return sp.horner(y, var)
        return mapS(lambda x: self.one(x, go), e)

    def cvDegrees(self, e, rest):
        return mapS(lambda x: self.one(x, lambda y: y * 180 / sp.pi), e)

    def cvRadians(self, e, rest):
        return mapS(lambda x: self.one(x, lambda y: y * sp.pi / 180), e)

    def cvBase(self, e, rest):
        n = intOf(e)
        b = intOf(rest[0])
        if b < 2:
            raise MapleError('invalid base')
        digs = []
        m = abs(n)
        while m:
            digs.append(sp.Integer(m % b))
            m //= b
        return MList(digs or [sp.Integer(0)])

    def cvBinary(self, e, rest):
        return sp.Integer(int(bin(intOf(e))[2:]))

    def cvConfrac(self, e, rest):
        return MList(list(sp.continued_fraction_iterator(e)))

    def cvVector(self, e, rest):
        if isinstance(e, Mat):
            return Mat(e.m, 'col' if e.m.cols == 1 else 'row')
        return Mat(sp.Matrix(list(e)), 'col')

    def cvMatrix(self, e, rest):
        if isinstance(e, Mat):
            return Mat(e.m, 'matrix')
        return Mat(sp.Matrix([list(r) if isinstance(r, (MList, list)) else [r] for r in e]), 'matrix')

    def cvName(self, e, rest):
        return sp.Symbol(str(e))

    def cvString(self, e, rest):
        return Str(sp.sstr(e))

    def cvExpsincos(self, e, rest):
        return mapS(lambda x: self.one(x, lambda y: y.rewrite(sp.exp).rewrite(sp.cos)), e)


def combineTrig(r):
    from sympy.simplify.fu import TR8, TR10i
    try:
        out = TR10i(TR8(r))
    except (ValueError, TypeError, NotImplementedError):
        out = r
    return out


CONVERT = {
    'parfrac': 'cvParfrac', 'fullparfrac': 'cvFullParfrac', 'exp': 'cvExp', 'trig': 'cvTrig',
    'sincos': 'cvSincos', 'tan': 'cvTan', 'ln': 'cvLn', 'polynom': 'cvPolynom', 'rational': 'cvRational',
    'float': 'cvFloat', 'list': 'cvListlike', 'set': 'cvSetlike', 'radical': 'cvRadical',
    'piecewise': 'cvPiecewise', 'Heaviside': 'cvHeaviside', 'binomial': 'cvBinomial',
    'factorial': 'cvFactorial', 'GAMMA': 'cvGamma', 'horner': 'cvHorner', 'degrees': 'cvDegrees',
    'radians': 'cvRadians', 'base': 'cvBase', 'binary': 'cvBinary', 'confrac': 'cvConfrac',
    'Vector': 'cvVector', 'Matrix': 'cvMatrix', 'name': 'cvName', 'string': 'cvString',
    'expsincos': 'cvExpsincos', 'sin': 'cvTrig', 'cos': 'cvTrig',
}

HANDLERS.update({
    'simplify': 'doSimplify', 'expand': 'doExpand', 'factor': 'doFactor', 'normal': 'doNormal',
    'combine': 'doCombine', 'collect': 'doCollect', 'sort': 'doSort', 'rationalize': 'doRationalize',
    'radnormal': 'doRadnormal', 'evalc': 'doEvalc', 'evalf': 'doEvalf', 'evalb': 'doEvalb',
    'is': 'doIs', 'testeq': 'doTesteq', 'convert': 'doConvert', 'subs': 'doSubs', 'eval': 'doEval',
})


# ----------------------------------------------------------------------------
# verify
# ----------------------------------------------------------------------------

def fmtPoint(v):
    """Short decimal text of a sample value (real or complex)."""
    re_, im_ = v.as_real_imag()
    def one(x):
        x = sp.Rational(x) if x.is_Rational else x
        if x.is_Integer:
            return str(int(x))
        f = float(x)
        return repr(round(f, 10)) if abs(f) > 1e-9 else '0'
    if im_ == 0:
        return one(re_)
    sign = '+' if im_ > 0 else '-'
    return '{}{}{}*I'.format(one(re_), sign, one(abs(im_)))


class VerifyMixin(object):
    """verify(lhs, rhs): symbolic zero test, then random-point numeric test at 40 digits."""

    def opVerify(self, req):
        self.setup(req.get('opts'))
        try:
            L = self.cv.toSympy(req['lhs'])
            R = self.cv.toSympy(req['rhs'])
        except MapleError as exc:
            if str(exc).startswith('numeric exception'):
                return {'ok': True, 'result': {'verdict': 'undetermined', 'method': 'error', 'note': str(exc)}}
            return {'ok': False, 'error': self.errMsg('verify', str(exc))}
        except (KeyError, TypeError, ValueError, IndexError) as exc:
            return {'ok': False, 'error': self.errMsg('verify', str(exc))}
        field = (req.get('opts') or {}).get('field')
        try:
            if field in FIELD_SETS:
                raws = ([req['lhs'], req.get('lhs_raw')], [req['rhs'], req.get('rhs_raw')])
                return {'ok': True, 'result': self.richVerify(L, R, field, req.get('opts'), raws)}
            res = self.verifyAny(L, R)
            out = {'verdict': res['verdict'], 'method': res['method']}
            if res.get('difference') is not None:
                out['difference'] = self.cv.toExpr(res['difference'])
            if res.get('counterexample'):
                out['counterexample'] = res['counterexample']
            if res.get('note'):
                out['note'] = res['note']
            return {'ok': True, 'result': out}
        except (EngineTimeout, RecursionError):
            return {'ok': True, 'result': {'verdict': 'undetermined', 'method': 'timeout'}}
        except Exception as exc:
            return {'ok': True, 'result': {'verdict': 'undetermined', 'method': 'error',
                                           'note': str(exc).split('\n')[0][:200]}}

    def res(self, verdict, method, **kw):
        d = {'verdict': verdict, 'method': method}
        d.update(kw)
        return d

    def verifyAny(self, L, R):
        if isinstance(L, Seq) and len(L) == 1:
            L = L[0]
        if isinstance(R, Seq) and len(R) == 1:
            R = R[0]
        if isinstance(L, (MList, Seq)) and isinstance(R, (MList, Seq)):
            return self.vWise(list(L), list(R))
        if isinstance(L, MSet) and isinstance(R, MSet):
            return self.vSet(L, R)
        if isinstance(L, Mat) and isinstance(R, Mat):
            if L.kind != R.kind or L.m.shape != R.m.shape:
                return self.res('not_equal', 'structure', note='different shape or kind')
            return self.vWise(list(L.m), list(R.m))
        if isRel(L) and isRel(R):
            return self.vRel(L, R)
        if isinstance(L, Rng) and isinstance(R, Rng):
            return self.vWise([L.lo, L.hi], [R.lo, R.hi])
        if isinstance(L, sp.Basic) and isinstance(R, sp.Basic) and not isRel(L) and not isRel(R):
            return self.verifyScalar(L, R)
        if type(L) is type(R) and not isinstance(L, sp.Basic):
            return self.res('equal' if sameObj(L, R) else 'not_equal', 'structure')
        return self.res('not_equal', 'structure', note='different kinds of objects')

    def combine(self, results):
        diffs = [r.get('difference') for r in results]
        for r in results:
            if r['verdict'] == 'not_equal':
                return r
        for r in results:
            if r['verdict'] == 'undetermined':
                return r
        method = 'symbolic' if all(r['method'] in ('symbolic', 'structure') for r in results) else 'numeric'
        return self.res('equal', method, difference=MList([d for d in diffs if d is not None]) if any(d is not None for d in diffs) else None)

    def vWise(self, ls, rs):
        if len(ls) != len(rs):
            return self.res('not_equal', 'structure', note='different lengths')
        return self.combine([self.verifyAny(x, y) for x, y in zip(ls, rs)])

    def vSet(self, L, R):
        if len(L) != len(R):
            return self.res('not_equal', 'structure', note='different number of elements')
        rest = list(R)
        results = []
        for x in L:
            found = None
            undet = None
            for y in rest:
                r = self.verifyAny(x, y)
                if r['verdict'] == 'equal':
                    found = (y, r)
                    break
                if r['verdict'] == 'undetermined' and undet is None:
                    undet = r
            if found:
                rest.remove(found[0])
                results.append(found[1])
            elif undet:
                results.append(undet)
            else:
                return self.res('not_equal', 'structure', note='sets differ')
        return self.combine(results)

    def vRel(self, L, R):
        o1, l1, r1 = relParts(L)
        o2, l2, r2 = relParts(R)
        if o1 != o2:
            return self.res('not_equal', 'structure', note='different relations')
        first = self.combine([self.verifyAny(l1, l2), self.verifyAny(r1, r2)])
        if first['verdict'] == 'equal' or o1 != 'eq':
            return first
        if not all(isinstance(t, sp.Basic) for t in (l1, r1, l2, r2)):
            return first
        d1, d2 = l1 - r1, l2 - r2
        ratio = self.constantRatio(d1, d2)
        if ratio is not None:
            return ratio
        return first

    def constantRatio(self, d1, d2):
        """Equations a=b and c=d are equivalent when a-b is a nonzero constant multiple of c-d."""
        ok, q = timeLimited(3, lambda: sp.simplify(d1 / d2))
        if ok and q is not None and isNum(q) and q != 0 and q.is_finite is not False:
            return self.res('equal', 'symbolic', note='equations equivalent up to a constant factor')
        syms = sorted(set(d1.free_symbols) | set(d2.free_symbols), key=lambda s: s.name)
        pts = self.samplePoints(syms, 8)
        ratios = []
        for pt in pts:
            v1, v2 = self.evalAt(d1, pt), self.evalAt(d2, pt)
            if v1 is None or v2 is None or abs(v2) < 1e-30:
                continue
            ratios.append(v1 / v2)
        if len(ratios) >= 3 and all(abs(r) > 1e-20 for r in ratios):
            if all(abs(r - ratios[0]) <= 1e-20 * max(1, abs(ratios[0])) for r in ratios):
                return self.res('equal', 'numeric', note='equations equivalent up to a constant factor')
        return None

    # ---- scalar ----

    def quickZero(self, d):
        if d == 0:
            return True
        if sp.expand(d) == 0:
            return True
        if sp.cancel(sp.together(d)) == 0:
            return True
        if sp.expand(d, trig=True) == 0:
            return True
        return False

    def deepZero(self, d):
        dd = d.doit() if d.has(sp.Integral, sp.Sum, sp.Product, sp.Derivative) else d
        steps = [sp.simplify, sp.trigsimp, sp.radsimp, sp.powsimp, sp.gammasimp, sp.hyperexpand,
                 lambda x: sp.simplify(sp.expand(x, trig=True)),
                 lambda x: sp.simplify(sp.expand_log(x, force=False)),
                 lambda x: sp.simplify(sp.expand_power_base(sp.expand(x))),
                 ]
        best = dd
        for st in steps:
            try:
                r = st(dd)
            except (ValueError, TypeError, NotImplementedError, ZeroDivisionError):
                continue
            if r == 0:
                return True, r
            if best == dd and r != dd and r.count_ops() < best.count_ops():
                best = r
        return False, best

    def verifyScalar(self, L, R):
        if L in (sp.true, sp.false) or R in (sp.true, sp.false):
            return self.res('equal' if L == R else 'not_equal', 'structure')
        d = L - R
        ok, z = timeLimited(3, self.quickZero, d)
        if ok and z:
            return self.res('equal', 'symbolic', difference=sp.Integer(0))
        num = self.numericTest(L, R)
        diff = d
        if num['verdict'] == 'not_equal':
            okS, bestS = timeLimited(2, sp.simplify, d)
            if okS and bestS is not None:
                diff = bestS
            return self.res('not_equal', 'numeric', difference=diff, counterexample=num['counterexample'])
        okD, zd = timeLimited(5, self.deepZero, d)
        if okD and zd and zd[0]:
            return self.res('equal', 'symbolic', difference=sp.Integer(0))
        if okD and zd and zd[1] is not None:
            diff = zd[1]
        if num['verdict'] == 'equal':
            kw = {'difference': diff}
            if num.get('note'):
                kw['note'] = num['note']
            return self.res('equal', 'numeric', **kw)
        return self.res('undetermined', num.get('method', 'numeric'), difference=diff)

    # ---- numeric machinery ----

    def undefAtoms(self, e):
        atoms = list(e.atoms(sp.core.function.AppliedUndef))
        atoms += list(e.atoms(sp.Derivative))
        atoms += list(e.atoms(sp.Subs))
        return atoms

    def drawReal(self, rng, s, lo, hi, integer):
        b = self.assume.bounds.get(s.name, {})
        loV, loS = (b['lo'][0], b['lo'][1]) if 'lo' in b else (None, False)
        hiV, hiS = (b['hi'][0], b['hi'][1]) if 'hi' in b else (None, False)
        lo_, hi_ = -3.0, 3.0
        if s.is_positive:
            lo_, hi_, loS = 0.1, 3.0, True
        elif s.is_nonnegative:
            lo_, hi_ = 0.0, 3.0
        elif s.is_negative:
            lo_, hi_, hiS = -3.0, -0.1, True
        elif s.is_nonpositive:
            lo_, hi_ = -3.0, 0.0
        if loV is not None:
            lo_ = max(lo_, float(loV)) if (s.is_positive or s.is_nonnegative) else float(loV)
            if hiV is None and hi_ < lo_ + 1:
                hi_ = lo_ + 3
        if hiV is not None:
            hi_ = min(hi_, float(hiV)) if (s.is_negative or s.is_nonpositive) else float(hiV)
            if loV is None and lo_ > hi_ - 1:
                lo_ = hi_ - 3
        if s.is_integer:
            a, bb = int(math.ceil(lo_)), int(math.floor(hi_))
            if s.is_positive or (loV is not None and loV >= 0 and loS):
                a = max(a, 1)
            if s.is_nonnegative:
                a = max(a, 0)
            if lo_ == -3.0 and hi_ == 3.0 and not (s.is_positive or s.is_nonnegative or s.is_negative or s.is_nonpositive):
                a, bb = -5, 5
            if s.is_positive and hi_ == 3.0:
                bb = 6
            if s.is_nonnegative and hi_ == 3.0:
                bb = 6
            if bb < a:
                bb = a
            val = rng.randint(a, bb)
            if s.is_even:
                val = 2 * (val // 2)
            if s.is_odd:
                val = 2 * (val // 2) + 1
            if s.is_nonzero or (s.is_zero is False):
                if val == 0:
                    val = 1 if (bb >= 1) else -1
            return sp.Integer(val)
        for _ in range(20):
            v = round(rng.uniform(lo_, hi_) * 10) / 10.0
            if v == 0 and not (s.is_nonnegative or s.is_nonpositive):
                continue
            if loS and loV is not None and v <= float(loV):
                continue
            if hiS and hiV is not None and v >= float(hiV):
                continue
            if s.is_positive and v <= 0:
                continue
            if s.is_negative and v >= 0:
                continue
            return sp.Rational(int(round(v * 10)), 10)
        return sp.Rational(int(round((lo_ + hi_) * 5)), 10) + sp.Rational(1, 10)

    def samplePoints(self, syms, n):
        rng = random.Random(20261027)
        pts = []
        for k in range(n):
            cplx = k >= (2 * n) // 3
            pt = {}
            for s in syms:
                realOnly = s.is_real or s.is_positive or s.is_negative or s.is_nonnegative or s.is_nonpositive \
                    or s.is_integer or s.is_rational
                if s.is_imaginary:
                    pt[s] = sp.I * sp.Rational(int(round(rng.uniform(-3, 3) * 10)) or 1, 10)
                elif cplx and not realOnly:
                    re_ = sp.Rational(int(round(rng.uniform(-1.4, 1.4) * 10)), 10)
                    im_ = sp.Rational(int(round(rng.uniform(-1.4, 1.4) * 10)) or 1, 10)
                    pt[s] = re_ + sp.I * im_
                else:
                    pt[s] = self.drawReal(rng, s, -3, 3, s.is_integer)
            pts.append(pt)
        return pts

    def evalAt(self, expr, pt):
        try:
            v = expr.xreplace(pt)
            v = sp.N(v, 40)
            if not v.is_number or v.has(sp.nan, sp.zoo, sp.oo, -sp.oo):
                return None
            re_, im_ = v.as_real_imag()
            if re_.has(sp.nan, sp.zoo, sp.oo) or im_.has(sp.nan, sp.zoo, sp.oo):
                return None
            return mpmath.mpc(mpmath.mpf(str(re_)), mpmath.mpf(str(im_)))
        except (TypeError, ValueError, ZeroDivisionError, NotImplementedError, AttributeError, OverflowError):
            return None

    def floatize(self, e):
        return exactFloats(e)

    def numericTest(self, L, R):
        ok, out = timeLimited(25, self.numericCore, L, R)
        if not ok or out is None:
            return {'verdict': 'undetermined', 'method': 'numeric'}
        return out

    def numericCore(self, L, R):
        subsBack = {}
        exprs = []
        nd = None
        for e in (L, R):
            e = e.doit() if e.has(sp.Integral, sp.Sum, sp.Product) else e
            e, n = self.floatize(e)
            if n is not None:
                nd = n if nd is None else min(nd, n)
            exprs.append(e)
        atoms = []
        if any(e.has(sp.Integral, sp.Sum, sp.Product, sp.Limit) and e.has(sp.core.function.AppliedUndef) for e in exprs):
            return {'verdict': 'undetermined', 'method': 'numeric'}
        for e in exprs:
            for at in self.undefAtoms(e):
                if at not in atoms:
                    atoms.append(at)
        rep = {}
        for at in sorted(atoms, key=lambda x: -x.count_ops() if hasattr(x, 'count_ops') else 0):
            dm = sp.Dummy('u')
            rep[at] = dm
            subsBack[dm] = at
        if rep:
            exprs = [self.replaceAtoms(e, rep) for e in exprs]
        syms = sorted(set().union(*[e.free_symbols for e in exprs]), key=lambda s: str(s))
        pts = self.samplePoints(syms, 12)
        floatMode = nd is not None
        if floatMode:
            nd = min(nd, self.digits)
            tolEq, tolNe = 10.0 ** (-(nd - 1)), 10.0 ** (-(nd - 4))
        else:
            tolEq, tolNe = 1e-25, 1e-20
        good = 0
        borderline = False
        for pt in pts:
            lv, rv = self.evalAt(exprs[0], pt), self.evalAt(exprs[1], pt)
            if lv is None or rv is None:
                continue
            good += 1
            diff = abs(lv - rv)
            scale = max(1, abs(lv))
            if diff > tolNe * max(scale, abs(rv)):
                ce = {}
                for s, val in pt.items():
                    key = s.name if isSym(s) and not isinstance(s, sp.Dummy) else self.atomKey(subsBack.get(s, s))
                    ce[key] = fmtPoint(val)
                return {'verdict': 'not_equal', 'method': 'numeric', 'counterexample': ce}
            if diff > tolEq * scale:
                borderline = True
        if good < 3 or borderline:
            return {'verdict': 'undetermined', 'method': 'numeric'}
        res = {'verdict': 'equal', 'method': 'numeric'}
        if floatMode:
            res['note'] = 'compared to {} significant digits'.format(nd)
        return res

    def replaceAtoms(self, e, rep):
        for at, dm in rep.items():
            e = e.xreplace({at: dm})
        return e

    def atomKey(self, at):
        if isinstance(at, sp.core.function.AppliedUndef):
            return '{}({})'.format(at.func.__name__, ', '.join(sp.sstr(x) for x in at.args))
        if isinstance(at, sp.Derivative):
            vs = []
            for v, n in at.variable_count:
                vs.extend([sp.sstr(v)] * int(n))
            return 'diff({}, {})'.format(self.atomKey(at.expr) if isinstance(at.expr, sp.core.function.AppliedUndef) else sp.sstr(at.expr), ', '.join(vs))
        return sp.sstr(at)


# ----------------------------------------------------------------------------
# domain analysis: where each side exists (used by verify, check_step and check_solution)
# ----------------------------------------------------------------------------

INERT_KINDS = (sp.Sum, sp.Product, sp.Integral)
RAW_SKIP = {'sum', 'Sum', 'int', 'Int', 'product', 'Product', 'limit', 'Limit', 'piecewise', 'diff', 'Diff', 'D'}
RAW_FUNCS = {'ln': sp.log, 'log': sp.log, 'log10': sp.log, 'log2': sp.log, 'tan': sp.tan, 'cot': sp.cot,
             'sec': sp.sec, 'csc': sp.csc, 'coth': sp.coth, 'csch': sp.csch, 'arcsin': sp.asin, 'arccos': sp.acos,
             'arcsec': sp.asec, 'arccsc': sp.acsc, 'arctanh': sp.atanh, 'arccoth': sp.acoth, 'arccosh': sp.acosh}
RAW_CONSTS = {'Pi', 'I', 'infinity'}
FIELD_SETS = {'real': sp.S.Reals, 'complex': sp.S.Complexes}
FAMILY_INDEX = '_Z1'


def negTerm(e):
    """Positive counterpart of a negative EXPR term, else None."""
    if not isinstance(e, dict):
        return None
    if 'int' in e and e['int'].startswith('-'):
        return {'int': e['int'][1:]}
    if 'rat' in e and e['rat'][0].startswith('-'):
        return {'rat': [e['rat'][0][1:], e['rat'][1]]}
    if 'float' in e and e['float'].startswith('-'):
        return {'float': e['float'][1:], 'digits': e.get('digits')}
    if 'mul' in e and e['mul']:
        nf = negTerm(e['mul'][0])
        if nf is not None:
            rest = e['mul'][1:]
            if nf.get('int') == '1':
                if not rest:
                    return nf
                return rest[0] if len(rest) == 1 else {'mul': rest}
            return {'mul': [nf] + rest}
    return None


def isNumNode(e):
    return isinstance(e, dict) and any(k in e for k in ('int', 'rat', 'float', 'const'))


def exprText(e, ctx=0):
    """Maple 1-D text of an EXPR tree. ctx: 0 top, 1 add term, 2 mul factor, 3 pow base, 4 exponent."""
    if not isinstance(e, dict):
        return str(e)
    if 'int' in e:
        s = e['int']
        return '({})'.format(s) if s.startswith('-') and ctx >= 2 else s
    if 'rat' in e:
        s = '{}/{}'.format(*e['rat'])
        return '({})'.format(s) if ctx >= 2 else s
    if 'float' in e:
        s = e['float']
        return '({})'.format(s) if s.startswith('-') and ctx >= 2 else s
    if 'sym' in e:
        return e['sym']
    if 'idx' in e:
        return '{}[{}]'.format(exprText(e['idx'][0]), ', '.join(exprText(i) for i in e['idx'][1]))
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
            n = negTerm(t)
            if n is not None:
                out += '-' + exprText(n, 1)
            else:
                out += ('+' if i else '') + exprText(t, 1)
        return '({})'.format(out) if ctx >= 2 else out
    if 'mul' in e:
        facs = list(e['mul'])
        sign = ''
        neg = negTerm(facs[0]) if facs else None
        if neg is not None:
            sign = '-'
            facs[0] = neg
            if neg.get('int') == '1' and len(facs) > 1:
                facs = facs[1:]
        nums, dens = [], []
        for f in facs:
            if 'pow' in f:
                ne = negTerm(f['pow'][1])
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
        out = '*'.join(exprText(n, 2) for n in nums) or '1'
        if dens:
            if len(dens) == 1:
                out += '/' + exprText(dens[0], 3)
            else:
                out += '/(' + '*'.join(exprText(x, 2) for x in dens) + ')'
        out = sign + out
        return '({})'.format(out) if ctx >= 2 and sign or ctx >= 3 else out
    if 'pow' in e:
        b, x = e['pow']
        if x.get('rat') == ['1', '2']:
            return 'sqrt({})'.format(exprText(b))
        ne = negTerm(x)
        if ne is not None:
            inner = b if ne.get('int') == '1' else {'pow': [b, ne]}
            s = '1/' + exprText(inner, 3)
            return '({})'.format(s) if ctx >= 2 else s
        s = '{}^{}'.format(exprText(b, 3), exprText(x, 4))
        return '({})'.format(s) if ctx >= 3 else s
    if 'fn' in e:
        head = exprText(e['fn']) if isinstance(e['fn'], dict) else e['fn']
        return '{}({})'.format(head, ', '.join(exprText(a) for a in e.get('args', [])))
    if 'fnidx' in e:
        n, idx, args = e['fnidx']
        return '{}[{}]({})'.format(n, ', '.join(exprText(i) for i in idx), ', '.join(exprText(a) for a in args))
    for k, sym, rev in (('eq', '=', '='), ('neq', '<>', '<>'), ('lt', '<', '>'), ('le', '<=', '>=')):
        if k in e:
            a, b = e[k]
            if k in ('lt', 'le') and isNumNode(a) and not isNumNode(b):
                return '{} {} {}'.format(exprText(b), rev, exprText(a))
            return '{} {} {}'.format(exprText(a), sym, exprText(b))
    if 'and' in e:
        return ' and '.join(exprText(t, 1) if 'or' in t else exprText(t) for t in e['and'])
    if 'or' in e:
        s = ' or '.join(exprText(t) for t in e['or'])
        return '({})'.format(s) if ctx >= 1 else s
    if 'not' in e:
        return 'not ({})'.format(exprText(e['not']))
    if 'list' in e:
        return '[' + ', '.join(exprText(t) for t in e['list']) + ']'
    if 'set' in e:
        return '{' + ', '.join(exprText(t) for t in e['set']) + '}'
    if 'seq' in e:
        return ', '.join(exprText(t) for t in e['seq'])
    if 'range' in e:
        return '{} .. {}'.format(exprText(e['range'][0]), exprText(e['range'][1]))
    return str(e)


def fmtValue(v):
    """Short text of an mpmath number (real or complex)."""
    def one(x):
        s = mpmath.nstr(x, 10)
        return s[:-2] if s.endswith('.0') else s
    if abs(v.imag) <= 1e-30 * max(1, abs(v)):
        return one(v.real)
    if abs(v.real) <= 1e-30 * max(1, abs(v)):
        return '{}*I'.format(one(v.imag))
    return '{}{}{}*I'.format(one(v.real), '+' if v.imag > 0 else '-', one(abs(v.imag)))


def hasFamily(S):
    return isinstance(S, sp.Basic) and S.has(sp.ImageSet)


def undoComplement(X):
    """Complement(A, Complement(A, Z)) -> Intersection(A, Z), also inside unions."""
    if isinstance(X, sp.Union):
        return sp.Union(*[undoComplement(a) for a in X.args])
    if isinstance(X, sp.Complement) and isinstance(X.args[1], sp.Complement) and X.args[0] == X.args[1].args[0]:
        Z = X.args[1].args[1]
        return Z if X.args[0] in (sp.S.Reals, sp.S.Complexes) else sp.Intersection(X.args[0], Z)
    return X


class DomainMixin(object):
    """Existence conditions of expressions over the reals or the complexes, and solution sets as text."""

    def mText(self, e):
        try:
            return exprText(self.cv.toExpr(e))
        except (MapleError, TypeError, ValueError, KeyError, AttributeError, IndexError):
            return sp.sstr(e)

    def newSym(self, s, extra, field):
        props = dict(self.assume.props.get(s.name, {}))
        if field == 'real' and not props.get('imaginary'):
            props['real'] = True
        props.update(extra)
        try:
            ns = sp.Symbol(s.name, **props)
            ns.is_real, ns.is_positive, ns.is_negative, ns.is_zero
            return ns
        except (ValueError, TypeError, InconsistentAssumptions):
            return None

    def fieldRep(self, exprs, field):
        """Real field: every free symbol becomes real (session assumptions kept)."""
        rep = {}
        if field != 'real':
            return rep
        for e in exprs:
            if not isinstance(e, sp.Basic):
                continue
            for s in e.free_symbols:
                if s in rep or not isinstance(s, sp.Symbol) or isinstance(s, (sp.Dummy, IdxSym)):
                    continue
                if s.is_real or s.is_imaginary:
                    continue
                ns = self.newSym(s, {}, field)
                if ns is not None:
                    rep[s] = ns
        return rep

    # ---- existence conditions ----

    def normCond(self, c, field):
        """Split and tidy one condition; returns a list of SymPy booleans (True ones dropped)."""
        if c is sp.true or c is True:
            return []
        if isinstance(c, sp.And):
            out = []
            for t in c.args:
                out.extend(self.normCond(t, field))
            return out
        if isinstance(c, sp.Ne):
            f = c.lhs - c.rhs
            if f.is_number:
                return [] if f.is_zero is False or (f.is_zero is None and abs(sp.N(f)) > 1e-30) else [sp.false]
            f = sp.factor_terms(f)
            if isinstance(f, sp.Mul):
                out = []
                for t in f.args:
                    if not t.is_number:
                        out.extend(self.normCond(sp.Ne(t, 0), field))
                return out
            if isinstance(f, sp.Pow) and f.exp.is_positive:
                return self.normCond(sp.Ne(f.base, 0), field)
            if isinstance(f, sp.Abs):
                return self.normCond(sp.Ne(f.args[0], 0), field)
            if f.could_extract_minus_sign():
                f = -f
            c = sp.Ne(f, 0)
        elif isinstance(c, (sp.StrictGreaterThan, sp.GreaterThan, sp.StrictLessThan, sp.LessThan)):
            big, small = c.gts, c.lts
            strict = isinstance(c, (sp.StrictGreaterThan, sp.StrictLessThan))
            if field == 'real' and strict and small == sp.Abs(sp.arg(small.args[0].args[0])) \
                    if isinstance(small, sp.Abs) and isinstance(small.args[0], sp.arg) else False:
                u = small.args[0].args[0]
                if u.is_real and big == sp.pi / 2:
                    # real u: arg(u) is 0 for u > 0 and Pi for u < 0
                    return self.normCond(sp.Gt(u, 0), field)
            if small == 0 and field == 'real':
                g = big
                if isinstance(g, sp.Pow) and g.exp.is_even:
                    return self.normCond(sp.Ne(g.base, 0), field) if strict else []
                if isinstance(g, sp.Abs):
                    return self.normCond(sp.Ne(g.args[0], 0), field) if strict else []
        if c is sp.true:
            return []
        return [c]

    def condsOf(self, e, field):
        """Existence conditions read from the expression tree."""
        out = []
        limits = []

        def add(c):
            for t in self.normCond(c, field):
                if t is sp.true:
                    continue
                if not any(t == o for o in out):
                    out.append(t)
        if not isinstance(e, sp.Basic):
            return out, limits
        it = sp.preorder_traversal(e)
        for sub in it:
            if isinstance(sub, INERT_KINDS):
                it.skip()
                continue
            if isinstance(sub, sp.Piecewise):
                good = [c for ex, c in sub.args if ex is not sp.nan]
                if len(good) < len(sub.args):
                    add(sp.Or(*good) if good else sp.false)
                if len(good) > 1:
                    limits.append('conditions inside the branches of a piecewise expression are not analysed')
                it.skip()
                continue
            self.nodeConds(sub, field, add)
        return out, limits

    def nodeConds(self, sub, field, add):
        """Existence conditions contributed by one node (not its children)."""
        if isinstance(sub, sp.Pow):
            b, x = sub.base, sub.exp
            if b.is_number and (b.is_positive or b == sp.E):
                return
            if x.is_integer:
                if not x.is_nonnegative:
                    add(sp.Ne(b, 0))
            elif x.is_number:
                if field == 'real':
                    add(sp.Gt(b, 0) if x.is_negative else sp.Ge(b, 0))
                elif x.is_negative:
                    add(sp.Ne(b, 0))
            elif not b.is_number:
                if field == 'real':
                    add(sp.Gt(b, 0))
                elif not x.is_positive:
                    add(sp.Ne(b, 0))
            return
        if isinstance(sub, sp.log):
            a = sub.args[0]
            add(sp.Gt(a, 0) if field == 'real' else sp.Ne(a, 0))
        elif isinstance(sub, (sp.tan, sp.sec)):
            add(sp.Ne(sp.cos(sub.args[0]), 0))
        elif isinstance(sub, (sp.cot, sp.csc)):
            add(sp.Ne(sp.sin(sub.args[0]), 0))
        elif isinstance(sub, (sp.coth, sp.csch)):
            a = sub.args[0]
            add(sp.Ne(a, 0) if field == 'real' else sp.Ne(sp.sinh(a), 0))
        elif isinstance(sub, (sp.asin, sp.acos)) and field == 'real':
            add(sp.Le(sp.Abs(sub.args[0]), 1))
        elif isinstance(sub, (sp.asec, sp.acsc)):
            a = sub.args[0]
            add(sp.Ge(sp.Abs(a), 1) if field == 'real' else sp.Ne(a, 0))
        elif isinstance(sub, sp.atanh):
            a = sub.args[0]
            if field == 'real':
                add(sp.Lt(sp.Abs(a), 1))
            else:
                add(sp.Ne(a, 1))
                add(sp.Ne(a, -1))
        elif isinstance(sub, sp.acoth):
            a = sub.args[0]
            if field == 'real':
                add(sp.Gt(sp.Abs(a), 1))
            else:
                add(sp.Ne(a, 1))
                add(sp.Ne(a, -1))
        elif isinstance(sub, sp.acosh) and field == 'real':
            add(sp.Ge(sub.args[0], 1))

    def rawConds(self, node, field):
        """Conditions read from the request tree before automatic simplification (x/x, sqrt(x)^2 ...)."""
        out = []

        def add(c):
            for t in self.normCond(c, field):
                if not any(t == o for o in out):
                    out.append(t)

        def fix(n):
            if isinstance(n, dict) and n.get('sym') in RAW_CONSTS and len(n) == 1:
                return {'const': n['sym']}
            if isinstance(n, dict):
                return {k: fix(v) for k, v in n.items()}
            if isinstance(n, list):
                return [fix(v) for v in n]
            return n

        def walk(n):
            if isinstance(n, list):
                for t in n:
                    walk(t)
                return
            if not isinstance(n, dict):
                return
            if isinstance(n.get('fn'), str) and n['fn'] in RAW_SKIP:
                return
            try:
                if isinstance(n.get('fn'), str) and n['fn'] in RAW_FUNCS and len(n.get('args', [])) == 1:
                    self.nodeConds(RAW_FUNCS[n['fn']](self.cv.toSympy(n['args'][0]), evaluate=False), field, add)
                elif isinstance(n.get('fn'), str) and n['fn'] == 'sqrt' and len(n.get('args', [])) == 1:
                    self.nodeConds(sp.Pow(self.cv.toSympy(n['args'][0]), sp.S.Half, evaluate=False), field, add)
                elif 'pow' in n:
                    b, x = [self.cv.toSympy(t) for t in n['pow']]
                    if isinstance(b, sp.Basic) and isinstance(x, sp.Basic):
                        self.nodeConds(sp.Pow(b, x, evaluate=False), field, add)
            except (MapleError, TypeError, ValueError, KeyError, IndexError, AttributeError, ZeroDivisionError):
                pass
            for v in n.values():
                walk(v)
        if node is not None:
            walk(fix(node))
        return out

    def boundsInterval(self, s):
        """Interval allowed for a real symbol by the declared assumptions (None when unconstrained)."""
        b = self.assume.bounds.get(s.name, {})
        lo, hi, loO, hiO = -sp.oo, sp.oo, True, True
        if s.is_positive:
            lo, loO = sp.Integer(0), True
        elif s.is_nonnegative:
            lo, loO = sp.Integer(0), False
        if s.is_negative:
            hi, hiO = sp.Integer(0), True
        elif s.is_nonpositive:
            hi, hiO = sp.Integer(0), False
        if 'lo' in b and b['lo'][0] >= lo:
            lo, loO = b['lo'][0], b['lo'][1]
        if 'hi' in b and b['hi'][0] <= hi:
            hi, hiO = b['hi'][0], b['hi'][1]
        if lo == -sp.oo and hi == sp.oo:
            return None
        return sp.Interval(lo, hi, loO, hiO)

    def pruneConds(self, conds):
        """Drop conditions that the declared bounds already guarantee (one-symbol conditions only)."""
        out = []
        for c in conds:
            if c is sp.true:
                continue
            fs = list(c.free_symbols)
            if len(fs) == 1 and fs[0].is_real:
                iv = self.boundsInterval(fs[0])
                if iv is not None:
                    try:
                        cs = self.condSet(c, fs[0], 'real')
                        if cs is not None and iv.is_subset(cs) is True:
                            continue
                    except (NotImplementedError, TypeError, ValueError, AttributeError):
                        pass
            out.append(c)
        return out

    def strengthenRep(self, conds, field):
        """Symbols restricted by simple conditions (x > 0, x <> 0, ...) get those assumptions."""
        props = {}
        for c in conds:
            s, p = None, None
            if isinstance(c, sp.Ne) and isSym(c.lhs) and c.rhs == 0:
                s, p = c.lhs, 'nonzero'
            elif isinstance(c, (sp.StrictGreaterThan, sp.GreaterThan, sp.StrictLessThan, sp.LessThan)):
                strict = isinstance(c, (sp.StrictGreaterThan, sp.StrictLessThan))
                if c.lts == 0 and isSym(c.gts):
                    s, p = c.gts, 'positive' if strict else 'nonnegative'
                elif c.gts == 0 and isSym(c.lts):
                    s, p = c.lts, 'negative' if strict else 'nonpositive'
            if s is None or isinstance(s, (sp.Dummy, IdxSym)):
                continue
            props.setdefault(s, {})[p] = True
        rep = {}
        for s, p in props.items():
            if p.get('nonnegative') and p.get('nonzero'):
                p = {'positive': True}
            if p.get('nonpositive') and p.get('nonzero'):
                p = {'negative': True}
            ns = self.newSym(s, dict(p, **({'real': True} if field == 'real' else {})), field)
            if ns is not None:
                rep[s] = ns
        return rep

    # ---- sums, products and integrals with their conditions ----

    def convergence(self, node):
        try:
            return node.is_convergent()
        except (NotImplementedError, ValueError, TypeError, AttributeError, IndexError):
            return None

    def resolveOne(self, node):
        """Closed form of one inert Sum/Product/Integral: (value, conds, notes, limits, status)."""
        txt = self.mText(node)
        kind = 'series' if isinstance(node, sp.Sum) else ('product' if isinstance(node, sp.Product) else 'integral')
        try:
            r = node.doit()
        except (NotImplementedError, ValueError, TypeError, AttributeError, ZeroDivisionError):
            r = node
        if r in (sp.oo, -sp.oo, sp.zoo) or r is sp.nan:
            to = ' to {}'.format(self.mText(r)) if r in (sp.oo, -sp.oo) else ''
            return r if to else sp.nan, [], ['the {} {} diverges{}'.format(kind, txt, to)], [], 'divergent'
        if isinstance(r, sp.Piecewise):
            good = [(ex, c) for ex, c in r.args if not ex.has(type(node)) and ex is not sp.nan]
            c0 = r.args[0][1]
            generic = isinstance(c0, sp.Ne) or (isinstance(c0, sp.And) and all(isinstance(t, sp.Ne) for t in c0.args))
            if len(good) == len(r.args) and generic:
                # generic case first (n <> -1 for x^n): keep it with its condition, as Maple's answer does
                rest = ['{} if {}'.format(self.mText(e), self.mText(c)) if c is not sp.true else
                        '{} otherwise'.format(self.mText(e)) for e, c in r.args[1:]]
                return r.args[0][0], [c0], ['{} = {} where {}; other case: {}'.format(
                    txt, self.mText(r.args[0][0]), self.mText(c0), ', '.join(rest))], [], 'ok'
            if len(good) == len(r.args):
                return r, [], [], [], 'ok'
            if not good:
                return node, [], [], ['could not decide where {} converges'.format(txt)], 'undecided'
            if len(good) == 1:
                ex, c = good[0]
                return ex, [c], ['{} = {} only where {} ({} condition)'.format(txt, self.mText(ex), self.mText(c),
                                 'convergence' if kind != 'integral' else 'existence')], [], 'cond:{}'.format(len(good))
            cond = sp.Or(*[c for _, c in good])
            return sp.Piecewise(*(good + [(sp.nan, True)])), [cond], [], [], 'cond:{}'.format(len(good))
        if r == node or r.has(type(node)):
            if kind == 'integral':
                return node, [], [], ['{} has no closed form here; only numeric checks were possible'.format(txt)], 'ok'
            conv = self.convergence(node)
            if conv is False or conv == sp.false:
                return sp.nan, [], ['the {} {} diverges'.format(kind, txt)], [], 'divergent'
            if conv is True or conv == sp.true:
                return node, [], ['{} converges; no closed form, compared numerically'.format(txt)], [], 'ok'
            return node, [], [], ['could not decide whether {} converges'.format(txt)], 'undecided'
        return r, [], [], [], 'ok'

    def boundaryReport(self, node, cond, field):
        """Convergence at the boundary and outside the SymPy condition, tested at a few points."""
        fs = [s for s in cond.free_symbols if isSym(s)]
        if field != 'real' or len(fs) != 1:
            return [], ['convergence outside {} was not analysed'.format(self.mText(cond))]
        s = fs[0]
        pts = set()
        for rel in cond.atoms(sp.core.relational.Relational):
            try:
                z = sp.solveset(sp.Eq(rel.lhs, rel.rhs), s, sp.S.Reals)
            except (NotImplementedError, ValueError, TypeError):
                continue
            if isinstance(z, sp.FiniteSet):
                pts.update(p for p in z if p.is_real)
        bnd = sorted(pts, key=lambda p: float(p))
        tests = list(bnd)
        if bnd:
            tests += [bnd[0] - 1, bnd[-1] + 1]
        div, conv, unk = [], [], []
        for p in tests:
            try:
                inside = bool(cond.xreplace({s: p}))
            except TypeError:
                continue
            v = self.convergence(node.xreplace({s: p}))
            label = '{} = {}'.format(s.name, self.mText(p)) + (' (boundary)' if p in pts else '')
            if inside:
                if p in pts and (v is True or v == sp.true):
                    conv.append(label)
                continue
            if v is False or v == sp.false:
                div.append(label)
            elif v is True or v == sp.true:
                conv.append(label + ' outside the condition')
            else:
                unk.append(label)
        notes, limits = [], []
        if div:
            notes.append('{} diverges at {}'.format(self.mText(node), ', '.join(div)))
        if conv:
            notes.append('{} converges at {}'.format(self.mText(node), ', '.join(conv)))
        if unk:
            limits.append('convergence of {} undecided at {}'.format(self.mText(node), ', '.join(unk)))
        limits.append('divergence outside {} was checked only at the listed points, not proved'.format(self.mText(cond)))
        return notes, limits

    def resolveInert(self, e, field):
        """Replace inert sums/products/integrals by closed forms, keeping their conditions."""
        info = {'conds': [], 'notes': [], 'limits': [], 'status': 'ok'}
        if not isinstance(e, sp.Basic):
            return e, info
        done = set()
        for _ in range(8):
            nodes = [n for n in e.atoms(*INERT_KINDS) if not n.function.has(*INERT_KINDS) and n not in done]
            if not nodes:
                break
            rep = {}
            for n in nodes:
                v, cs, notes, limits, st = self.resolveOne(n)
                rep[n] = v
                done.add(v)
                info['conds'] += cs
                info['notes'] += notes
                info['limits'] += limits
                if st == 'divergent':
                    info['status'] = 'divergent'
                    if v in (sp.oo, -sp.oo) and n == e:
                        info['infinite'] = v
                elif st == 'undecided' and info['status'] == 'ok':
                    info['status'] = 'undecided'
                for c in (cs if st.startswith('cond') and not isinstance(n, sp.Integral) else []):
                    if not self.pruneConds([c]):
                        continue
                    bn, bl = self.boundaryReport(n, c, field)
                    info['notes'] += bn
                    info['limits'] += bl
            e = e.xreplace(rep)
        return e, info

    # ---- sets ----

    def fieldSet(self, s, field):
        base = FIELD_SETS[field]
        if field == 'real' and s.is_integer:
            base = sp.S.Integers
        iv = self.boundsInterval(s) if field == 'real' else None
        return base if iv is None else sp.Intersection(base, iv)

    def condSet(self, c, s, field):
        """Set of values of s where condition c holds (None when SymPy cannot say)."""
        base = FIELD_SETS[field]
        if c is sp.true:
            return base
        if c is sp.false:
            return sp.S.EmptySet
        if isinstance(c, sp.Ne):
            z = sp.solveset(sp.Eq(c.lhs, c.rhs), s, base)
            if isinstance(z, sp.ConditionSet):
                return None
            return sp.Complement(base, z)
        if isinstance(c, sp.And):
            parts = [self.condSet(t, s, field) for t in c.args]
            return None if any(p is None for p in parts) else sp.Intersection(*parts)
        if isinstance(c, sp.Or):
            parts = [self.condSet(t, s, field) for t in c.args]
            return None if any(p is None for p in parts) else sp.Union(*parts)
        if field != 'real':
            return None
        r = sp.solveset(c, s, sp.S.Reals)
        return None if isinstance(r, sp.ConditionSet) or r.has(sp.ConditionSet) else r

    def mergeFamilies(self, sets, n):
        """Join ImageSets p*n + c_i whose offsets are equally spaced (2*Pi*n, 2*Pi*n+Pi -> Pi*n)."""
        groups = {}
        rest = []
        for S in sets:
            lam = S.lamda
            if S.base_sets != (sp.S.Integers,) or len(lam.variables) != 1:
                rest.append(S)
                continue
            body = sp.expand(lam.expr.xreplace({lam.variables[0]: n}))
            p = body.coeff(n)
            c = sp.expand(body - p * n)
            if p == 0 or c.has(n):
                rest.append(S)
                continue
            groups.setdefault(p, []).append(c)
        out = []
        for p, cs in groups.items():
            cs = sorted(set(cs), key=lambda c: float(sp.N(c)) if c.is_number else 0)
            k = len(cs)
            step = p / k
            if k > 1 and all(sp.simplify(cs[i + 1] - cs[i] - step) == 0 for i in range(k - 1)):
                out.append(step * n + cs[0])
            else:
                out.extend(p * n + c for c in cs)
        return out, rest

    def setText(self, S, s):
        """Readable Maple-style description of a solution or exclusion set; returns (text, kind)."""
        if S is sp.S.EmptySet:
            return 'none', 'empty'
        if isinstance(S, sp.FiniteSet):
            return ', '.join('{} = {}'.format(s.name, self.mText(p)) for p in S), 'finite'
        if S in (sp.S.Reals, sp.S.Complexes):
            return 'every {} {}'.format('real' if S == sp.S.Reals else 'complex', s.name), 'all'
        if isinstance(S, sp.Union) and all(isinstance(a, sp.Complement) for a in S.args) \
                and len(set(a.args[1] for a in S.args)) == 1:
            S = sp.Complement(sp.Union(*[a.args[0] for a in S.args], evaluate=False), S.args[0].args[1], evaluate=False)
        if isinstance(S, sp.Complement) and isinstance(S.args[1], sp.FiniteSet):
            a, ka = self.setText(S.args[0], s)
            b, _ = self.setText(S.args[1], s)
            return '{} except {}'.format(a, b), ka
        if isinstance(S, sp.ConditionSet):
            return 'not solved (solveset returned a ConditionSet)', 'unsolved'
        parts = list(S.args) if isinstance(S, sp.Union) else [S]
        fin, fams, other = [], [], []
        imgs = []
        for P in parts:
            if isinstance(P, sp.FiniteSet):
                fin.extend(P)
            elif isinstance(P, sp.ImageSet):
                imgs.append(P)
            else:
                other.append(P)
        if imgs:
            n = sp.Symbol(FAMILY_INDEX, integer=True)
            merged, rest = self.mergeFamilies(imgs, n)
            fams = ['{} = {}'.format(s.name, self.mText(m)) for m in merged]
            other += rest
        texts = ['{} = {}'.format(s.name, self.mText(p)) for p in fin] + fams
        for P in other:
            try:
                texts.append(self.mText(P.as_relational(s)))
            except (NotImplementedError, TypeError, AttributeError, ValueError):
                texts.append(sp.sstr(P))
        txt = (' or ' if other else ', ').join(texts)
        if fams:
            txt += ' ({} integer)'.format(FAMILY_INDEX)
            return txt, 'family'
        return txt, ('finite' if not other else 'set')

    def condTexts(self, c, field):
        """Maple text(s) of one condition; a nonzero condition with finitely many zeros becomes x <> a."""
        fs = [s for s in c.free_symbols if isSym(s)]
        if isinstance(c, sp.Ne) and len(fs) == 1:
            s = fs[0]
            try:
                z = sp.solveset(sp.Eq(c.lhs, c.rhs), s, FIELD_SETS[field])
            except (NotImplementedError, ValueError, TypeError):
                z = None
            if isinstance(z, sp.FiniteSet) and len(z) <= 6:
                return ['{} <> {}'.format(s.name, self.mText(p)) for p in z], None
            if z is not None and hasFamily(z):
                txt, _ = self.setText(z, s)
                return [self.mText(c)], 'excludes {}'.format(txt)
        if isinstance(c, sp.core.relational.Relational) and not isinstance(c, (sp.Ne, sp.Eq)) and len(fs) == 1 \
                and field == 'real':
            s = fs[0]
            d = c.lhs - c.rhs
            if d.is_polynomial(s) and sp.degree(d, s) == 1 and not (c.lhs == s and c.rhs.is_number):
                try:
                    r = sp.solveset(c, s, sp.S.Reals)
                    if isinstance(r, sp.Interval):
                        return [self.mText(r.as_relational(s))], None
                except (NotImplementedError, ValueError, TypeError):
                    pass
        return [self.mText(c)], None


# ----------------------------------------------------------------------------
# verify with domains (MCP check tools)
# ----------------------------------------------------------------------------

SUGGEST_MENU = [('nonnegative', '{} >= 0'), ('positive', '{} > 0'), ('negative', '{} < 0'), ('integer', '{}::integer')]


class RigorMixin(object):
    """verify over a field with domain conditions, convergence, and assumption suggestions."""

    def assumeTexts(self, opts):
        out = []
        for node in (opts or {}).get('assume') or []:
            if isinstance(node, dict) and node.get('fn') == 'type' and len(node.get('args', [])) == 2:
                out.append('{}::{}'.format(exprText(node['args'][0]), exprText(node['args'][1])))
            else:
                out.append(exprText(node))
        return out

    def truthAt(self, c, pt):
        """Truth of a condition at a numeric point: True, False or None."""
        try:
            v = c.xreplace(pt)
        except (TypeError, ValueError):
            return None
        if v is sp.true or v is True:
            return True
        if v is sp.false or v is False:
            return False
        if isinstance(v, sp.And):
            vals = [self.truthAt(t, {}) for t in v.args]
            return False if False in vals else (True if all(x is True for x in vals) else None)
        if isinstance(v, sp.Or):
            vals = [self.truthAt(t, {}) for t in v.args]
            return True if True in vals else (False if all(x is False for x in vals) else None)
        if isinstance(v, sp.core.relational.Relational):
            try:
                a = sp.N(v.lhs - v.rhs, 30)
                re_, im_ = a.as_real_imag()
                if not (re_.is_number and im_.is_number):
                    return None
                if isinstance(v, sp.Ne):
                    return abs(complex(a)) > 1e-25
                if isinstance(v, sp.Eq):
                    return abs(complex(a)) <= 1e-25
                if abs(float(im_)) > 1e-25:
                    return False
                f = float(re_)
                if isinstance(v, sp.StrictLessThan):
                    return f < -1e-25
                if isinstance(v, sp.LessThan):
                    return f <= 1e-25
                if isinstance(v, sp.StrictGreaterThan):
                    return f > 1e-25
                if isinstance(v, sp.GreaterThan):
                    return f >= -1e-25
            except (TypeError, ValueError, ZeroDivisionError, AttributeError):
                return None
        return None

    def isReal(self, v):
        return abs(v.imag) <= 1e-30 * max(1, abs(v))

    def inDomain(self, v, conds, pt, field):
        if v is None:
            return False
        if field == 'real' and not self.isReal(v):
            return False
        return all(self.truthAt(c, pt) is True for c in conds)

    def smallPoints(self, syms, n):
        """Extra sample points in (-1, 1) for domains such as abs(x) < 1."""
        rng = random.Random(4242)
        pts = []
        for _ in range(n):
            pt = {}
            for s in syms:
                if s.is_integer:
                    pt[s] = self.drawReal(rng, s, -3, 3, True)
                    continue
                k = rng.randint(1, 9)
                if s.is_negative or (not (s.is_positive or s.is_nonnegative) and rng.random() < 0.5):
                    k = -k
                pt[s] = sp.Rational(k, 10)
            pts.append(pt)
        return pts

    def numericDomainCore(self, L, R, condsL, condsR, field):
        exprs, nd = [], None
        for e in (L, R):
            e, n = self.floatize(e)
            if n is not None:
                nd = n if nd is None else min(nd, n)
            exprs.append(e)
        if any(e.has(*INERT_KINDS, sp.Limit) and e.has(sp.core.function.AppliedUndef) for e in exprs):
            return {'verdict': 'undetermined'}
        atoms = []
        for e in exprs:
            for at in self.undefAtoms(e):
                if at not in atoms:
                    atoms.append(at)
        rep, back = {}, {}
        for at in sorted(atoms, key=lambda x: -x.count_ops() if hasattr(x, 'count_ops') else 0):
            dm = sp.Dummy('u', real=True) if field == 'real' else sp.Dummy('u')
            rep[at] = dm
            back[dm] = at
        if rep:
            exprs = [self.replaceAtoms(e, rep) for e in exprs]
            condsL = [self.replaceAtoms(c, rep) for c in condsL]
            condsR = [self.replaceAtoms(c, rep) for c in condsR]
        syms = sorted(set().union(*[e.free_symbols for e in exprs + condsL + condsR]), key=lambda s: str(s))
        pts = self.samplePoints(syms, 36)
        real, cplx = pts[:24], pts[24:]
        pts = []
        while real or cplx:
            pts += real[:2] + cplx[:1]
            real, cplx = real[2:], cplx[1:]
        pts += self.smallPoints(syms, 20)
        if nd is not None:
            nd = min(nd, self.digits)
            tolEq, tolNe = 10.0 ** (-(nd - 1)), 10.0 ** (-(nd - 4))
        else:
            tolEq, tolNe = 1e-25, 1e-20

        def key(s):
            return s.name if isSym(s) and not isinstance(s, sp.Dummy) else self.atomKey(back.get(s, s))

        def where(pt):
            return {key(s): fmtPoint(v) for s, v in pt.items()}
        good, borderline, witness, gap = 0, False, None, None
        seen = set()
        for pt in pts:
            if good >= 12:
                break
            tag = tuple(sorted((str(k), str(v)) for k, v in pt.items()))
            if tag in seen:
                continue
            seen.add(tag)
            lv, rv = self.evalAt(exprs[0], pt), self.evalAt(exprs[1], pt)
            inL = self.inDomain(lv, condsL, pt, field)
            inR = self.inDomain(rv, condsR, pt, field)
            if inL and inR:
                good += 1
                diff = abs(lv - rv)
                scale = max(1, abs(lv), abs(rv))
                if diff > tolNe * scale:
                    return {'verdict': 'not_equal', 'counterexample': where(pt),
                            'lhs_value': fmtValue(lv), 'rhs_value': fmtValue(rv)}
                if diff > tolEq * scale:
                    borderline = True
                continue
            if inL == inR:
                continue
            one, other = (lv, rv) if inL else (rv, lv)
            if other is not None and abs(one - other) > tolNe * max(1, abs(one), abs(other)):
                if witness is None and field == 'real':
                    witness = {'point': where(pt), 'lhs_value': fmtValue(lv), 'rhs_value': fmtValue(rv),
                               'real_side': 'lhs' if inL else 'rhs'}
            elif other is None and gap is None:
                gap = where(pt)
        need = 3 if syms else 1
        out = {'verdict': 'undetermined' if good < need or borderline else 'equal', 'good': good}
        if witness:
            out['witness'] = witness
        if gap:
            out['gap'] = gap
        if nd is not None and out['verdict'] == 'equal':
            out['note'] = 'compared to {} significant digits'.format(nd)
        return out

    def symbolicZero(self, d, quick=3, deep=5):
        ok, z = timeLimited(quick, self.quickZero, d)
        if ok and z:
            return True, sp.Integer(0)
        okD, zd = timeLimited(deep, self.deepZero, d)
        if okD and zd and zd[0]:
            return True, sp.Integer(0)
        return False, (zd[1] if okD and zd and zd[1] is not None else d)

    def prepareSide(self, e, field, raw=()):
        """Resolve inert calculus and collect existence conditions of one side."""
        e1, info = self.resolveInert(e, field)
        conds, limits = self.condsOf(e1, field)
        allc = []
        for c in list(raw) + info['conds'] + conds:
            for t in self.normCond(c, field):
                if not any(t == o for o in allc):
                    allc.append(t)
        allc = self.pruneConds(allc)
        info['limits'] += limits
        info['condList'] = allc
        return e1, info

    def dropImplied(self, conds, s, field):
        """Remove conditions implied by the others (one-variable case: x <> 0 next to x > 0)."""
        keep = list(conds)
        for c in reversed(list(conds)):
            others = [o for o in keep if o is not c]
            if not others or c not in keep:
                continue
            try:
                sc = self.condSet(c, s, field)
                so = [self.condSet(o, s, field) for o in others]
                if sc is None or any(x is None for x in so):
                    continue
                if sp.Intersection(self.fieldSet(s, field), *so).is_subset(sc) is True:
                    keep.remove(c)
            except (NotImplementedError, TypeError, ValueError, AttributeError):
                continue
        return keep

    def domainInfo(self, condsL, condsR, common, field, syms, gap, witness):
        """Text of the domains, the conditions and whether both sides share the same natural domain."""
        texts, extra = [], []
        if len(syms) == 1:
            common = self.dropImplied(common, syms[0], field)
            condsL = self.dropImplied(condsL, syms[0], field)
            condsR = self.dropImplied(condsR, syms[0], field)
        for c in common:
            ts, ex = self.condTexts(c, field)
            for t in ts:
                if t not in texts:
                    texts.append(t)
            if ex:
                extra.append('{}: {}'.format(self.mText(c), ex))
        whole = 'all real numbers' if field == 'real' else 'all complex numbers'
        if self.assume.props or self.assume.bounds:
            whole = 'every value allowed by the assumptions'

        def side(cs):
            out = []
            for c in cs:
                out += self.condTexts(c, field)[0]
            return ' and '.join(out) if out else whole
        dom = {'lhs': side(condsL), 'rhs': side(condsR), 'common': ' and '.join(texts) if texts else whole}
        same = None
        sl = set(self.mText(c) for c in condsL)
        sr = set(self.mText(c) for c in condsR)
        if sl == sr:
            same = True
        elif len(syms) == 1:
            s = syms[0]
            base = self.fieldSet(s, field)
            try:
                dl = [self.condSet(c, s, field) for c in condsL]
                dr = [self.condSet(c, s, field) for c in condsR]
                if not any(x is None for x in dl + dr):
                    DL = sp.Intersection(base, *dl) if dl else base
                    DR = sp.Intersection(base, *dr) if dr else base
                    diff = undoComplement(sp.Union(sp.Complement(DL, DR), sp.Complement(DR, DL)))
                    if diff is sp.S.EmptySet:
                        same = True
                    else:
                        same = False
                        txt, _ = self.setText(diff, s)
                        if not isinstance(diff, (sp.Complement, sp.Intersection)):
                            dom['differ_at'] = txt
            except (NotImplementedError, TypeError, ValueError, AttributeError, RecursionError):
                same = None
        if same is None and (gap or witness):
            same = False
        dom['same_domain'] = same
        return texts, extra, dom

    def verifyRich(self, L, R, field, suggest=True, rawL=None, rawR=None):
        """verify lhs = rhs over a field: equality on the common domain, with its conditions."""
        start = time.time()
        okR, raws = timeLimited(5, lambda: (self.rawConds(rawL, field), self.rawConds(rawR, field)))
        cL, cR = raws if okR else ([], [])
        rep = self.fieldRep([L, R] + cL + cR, field)
        L0, R0 = L.xreplace(rep), R.xreplace(rep)
        cL, cR = [c.xreplace(rep) for c in cL], [c.xreplace(rep) for c in cR]
        out = {'field': field, 'notes': [], 'limitations': []}
        ok, sides = timeLimited(20, lambda: (self.prepareSide(L0, field, cL), self.prepareSide(R0, field, cR)))
        if not ok:
            out.update({'verdict': 'undetermined', 'method': 'timeout', 'conditions': []})
            out['limitations'].append('the domain analysis timed out')
            return out
        (L1, iL), (R1, iR) = sides
        for i in (iL, iR):
            out['notes'] += i['notes']
            out['limitations'] += i['limits']
        condsL, condsR = iL['condList'], iR['condList']
        if sp.false in condsL or sp.false in condsR:
            bad = 'lhs' if sp.false in condsL else 'rhs'
            out.update({'verdict': 'undetermined', 'method': 'domain', 'conditions': []})
            out['notes'].append('{} is not defined anywhere over the {}s'.format(bad, field))
            return out
        common = list(condsL) + [c for c in condsR if not any(c == o for o in condsL)]
        infL, infR = iL.get('infinite', L1), iR.get('infinite', R1)
        if 'divergent' in (iL['status'], iR['status']) and infL in (sp.oo, -sp.oo) and infL == infR:
            out.update({'verdict': 'equal', 'method': 'symbolic', 'conditions': []})
            out['notes'].append('both sides are {} (divergent)'.format(self.mText(infL)))
            return out
        if 'divergent' in (iL['status'], iR['status']):
            both = iL['status'] == iR['status']
            out.update({'verdict': 'undetermined' if both else 'not_equal', 'method': 'convergence',
                        'conditions': [self.mText(c) for c in common]})
            out['notes'].append('both sides diverge' if both else '{} diverges, so it has no value; the other side does'
                                .format('lhs' if iL['status'] == 'divergent' else 'rhs'))
            return out
        srep = self.strengthenRep(common, field)
        Ls, Rs = L1.xreplace(srep), R1.xreplace(srep)
        symEq, diff = self.symbolicZero(Ls - Rs)
        undecided = 'undecided' in (iL['status'], iR['status'])
        # a series of undecided convergence is not sampled: its numeric value could be meaningless
        num = self.numericTest2(L1, R1, condsL, condsR, field) if not undecided else {'verdict': 'undetermined'}
        syms = sorted({s for t in [L1, R1] + common for s in t.free_symbols if isSym(s)}, key=lambda s: s.name)
        method = 'symbolic' if symEq else 'numeric'
        if num['verdict'] == 'not_equal':
            if symEq:
                out.update({'verdict': 'undetermined', 'method': 'symbolic'})
                out['notes'].append('symbolic simplification gives 0 but a sampled point disagrees')
            else:
                out.update({'verdict': 'not_equal', 'method': 'numeric', 'counterexample': num['counterexample'],
                            'lhs_value': num['lhs_value'], 'rhs_value': num['rhs_value']})
                out['notes'].append('the counterexample lies inside the common domain')
        elif symEq or num['verdict'] == 'equal':
            if num.get('witness'):
                w = num['witness']
                other = 'rhs' if w['real_side'] == 'lhs' else 'lhs'
                out.update({'verdict': 'not_equal', 'method': 'numeric', 'outside_domain': w})
                diff = L1 - R1
                out['notes'].append('the sides agree on the common real domain, but at {} the {} is real and in its '
                                    'domain while the {} is not real (outside its real domain) and has a different '
                                    'value'.format(', '.join('{} = {}'.format(k, v) for k, v in w['point'].items()),
                                                   w['real_side'], other))
            else:
                out.update({'verdict': 'equal', 'method': method})
                if method == 'numeric' and num.get('note'):
                    out['notes'].append(num['note'])
        else:
            out.update({'verdict': 'undetermined', 'method': 'numeric'})
        if undecided and out['verdict'] == 'undetermined':
            out['method'] = 'convergence'
        if undecided and out['verdict'] == 'equal':
            out['verdict'] = 'undetermined'
            out['notes'].append('equal where the series converges, but convergence could not be decided')
        if diff is not None:
            out['difference'] = diff
        okD, dres = timeLimited(10, self.domainInfo, condsL, condsR, common, field, syms, num.get('gap'),
                                num.get('witness'))
        if okD:
            texts, extra, dom = dres
            out['conditions'] = texts
            out['domain'] = dom
            out['notes'] += extra
            w = out.get('outside_domain')
            if w:
                other = 'rhs' if w['real_side'] == 'lhs' else 'lhs'
                out['notes'].append('over the reals the {} requires {}, the {} only {}'.format(
                    other, dom[other], w['real_side'], dom[w['real_side']]))
            if dom['same_domain'] is False and out['verdict'] == 'equal':
                out['notes'].append('equal where both sides are defined; the domains differ{}'.format(
                    ' where ' + dom['differ_at'] if dom.get('differ_at') else ''))
            if dom['same_domain'] is None and len(syms) > 0 and common:
                out['limitations'].append('could not decide whether both sides have the same domain')
        else:
            out['conditions'] = [self.mText(c) for c in common]
            out['limitations'].append('the domain description timed out; conditions are listed unsolved')
        if out['verdict'] == 'equal' and method == 'numeric':
            out['limitations'].append('equality on the domain was checked numerically at sample points, not proved')
        if suggest and out['verdict'] != 'equal' and time.time() - start < 30:
            sug = self.suggest(L, R, field, syms)
            if sug:
                out['suggestion'] = sug
        return out

    def numericTest2(self, L, R, condsL, condsR, field, budget=25):
        ok, res = timeLimited(budget, self.numericDomainCore, L, R, condsL, condsR, field)
        if not ok or res is None:
            return {'verdict': 'undetermined'}
        return res

    def proveUnder(self, L, R, field, rep):
        """Symbolic-only proof of L = R on the common domain after replacing symbols (for suggestions)."""
        L1, R1 = L.xreplace(rep), R.xreplace(rep)
        frep = self.fieldRep([L1, R1], field)
        L1, R1 = L1.xreplace(frep), R1.xreplace(frep)
        ok, sides = timeLimited(4, lambda: (self.prepareSide(L1, field), self.prepareSide(R1, field)))
        if not ok:
            return None
        (A, iA), (B, iB) = sides
        if 'divergent' in (iA['status'], iB['status']) or 'undecided' in (iA['status'], iB['status']):
            return None
        common = iA['condList'] + [c for c in iB['condList'] if not any(c == o for o in iA['condList'])]
        if sp.false in common:
            return None
        srep = self.strengthenRep(common, field)
        z, _ = self.symbolicZero(A.xreplace(srep) - B.xreplace(srep), 1, 3)
        if not z:
            return None
        if field == 'real':
            num = self.numericTest2(A, B, iA['condList'], iB['condList'], field, 4)
            if num.get('witness') or num['verdict'] == 'not_equal':
                return None
        return [self.mText(c) for c in common]

    def suggest(self, L, R, field, syms):
        """Try a small menu of assumptions; keep the weakest that gives a symbolic proof."""
        free = [s for s in syms if not isinstance(s, (sp.Dummy, IdxSym))]
        if not free or len(free) > 2:
            return None
        start = time.time()
        byName = {}
        for e in (L, R):
            for s in e.free_symbols:
                if isSym(s):
                    byName[s.name] = s
        trials = []
        for prop, txt in SUGGEST_MENU:
            for s in free:
                if s.name in byName:
                    trials.append(([(byName[s.name], prop)], txt.format(s.name)))
            if len(free) == 2 and all(s.name in byName for s in free):
                trials.append(([(byName[s.name], prop) for s in free], ' and '.join(txt.format(s.name) for s in free)))
        proved = []
        for pairs, txt in trials:
            if time.time() - start > 12:
                break
            rep = {}
            for s, prop in pairs:
                if getattr(s, 'is_' + prop):
                    rep = None
                    break
                ns = self.newSym(s, {prop: True}, field)
                if ns is None:
                    rep = None
                    break
                rep[s] = ns
            if not rep:
                continue
            conds = self.proveUnder(L, R, field, rep)
            if conds is not None:
                proved.append((txt, conds))
        if not proved:
            return None
        # the assumption that leaves the fewest extra conditions wins; menu order breaks ties
        txt, conds = min(proved, key=lambda p: len(p[1]))
        line = 'equal if {}'.format(txt)
        if conds:
            line += ' (where {})'.format(' and '.join(conds))
        return line + ' -- found by trying a fixed menu of assumptions; the equality under it is a symbolic proof'

    def richOut(self, res, opts):
        """Engine JSON of a rich result: Maple strings, EXPR difference, legacy note field."""
        out = {}
        for k, v in res.items():
            if k in ('difference',):
                if isinstance(v, sp.Basic):
                    out[k] = self.cv.toExpr(v)
            elif k != 'notes':
                out[k] = v
        out.setdefault('verdict', 'undetermined')
        out.setdefault('method', 'numeric')
        out.setdefault('conditions', [])
        out.setdefault('limitations', [])
        out['assumptions_used'] = self.assumeTexts(opts)
        notes = [n for n in res.get('notes', []) if n]
        if notes:
            out['notes'] = notes
            out['note'] = '; '.join(notes)
        return out

    def richVerify(self, L, R, field, opts, raws=(None, None)):
        if isinstance(L, Seq) and len(L) == 1:
            L = L[0]
        if isinstance(R, Seq) and len(R) == 1:
            R = R[0]
        scalar = all(isinstance(t, sp.Basic) and not isRel(t) and t not in (sp.true, sp.false) for t in (L, R))
        if scalar:
            return self.richOut(self.verifyRich(L, R, field, rawL=raws[0], rawR=raws[1]), opts)
        res = self.verifyAny(L, R)
        res['field'] = field
        res['limitations'] = ['domain conditions are analysed only for scalar expressions; lists, sets, '
                              'equations and matrices are compared elementwise without them']
        if res.get('note'):
            res['notes'] = [res.pop('note')]
        return self.richOut(res, opts)


# ----------------------------------------------------------------------------
# check_step and check_solution
# ----------------------------------------------------------------------------

def eqSides(x):
    """Equation -> (lhs, rhs); an expression e means e = 0; None for other relations."""
    if isRel(x):
        op, l, r = relParts(x)
        return (l, r) if op == 'eq' else None
    return (x, sp.Integer(0))


def nonEmpty(X):
    """True/False when it is clear whether a set is empty, else None."""
    if X is sp.S.EmptySet:
        return False
    if isinstance(X, (sp.FiniteSet, sp.Interval, sp.ImageSet)) or X in (sp.S.Reals, sp.S.Complexes, sp.S.Integers):
        return True
    if isinstance(X, sp.Union):
        vals = [nonEmpty(a) for a in X.args]
        return True if True in vals else (False if all(v is False for v in vals) else None)
    if isinstance(X, sp.Complement) and isinstance(X.args[1], sp.FiniteSet):
        a = X.args[0]
        if a.is_finite_set is False or hasFamily(a) or isinstance(a, sp.Interval):
            return True
    return None


class StepMixin(object):
    """check_step: equivalent / implies / implied_by / not_equivalent; check_solution with domains."""

    def fieldOpt(self, opts):
        f = (opts or {}).get('field')
        return f if f in FIELD_SETS else 'real'

    def opCheckStep(self, req):
        opts = req.get('opts') or {}
        self.setup(opts)
        field = self.fieldOpt(opts)
        try:
            B = self.cv.toSympy(req['before'])
            A = self.cv.toSympy(req['after'])
            V = self.cv.toSympy(req['var']) if req.get('var') else None
        except (MapleError, KeyError, TypeError, ValueError, IndexError) as exc:
            return {'ok': False, 'error': self.errMsg('check_step', str(exc))}
        try:
            raws = ([req['before'], req.get('before_raw')], [req['after'], req.get('after_raw')])
            res = self.checkStep(B, A, V, field, raws)
        except MapleError as exc:
            return {'ok': False, 'error': self.errMsg('check_step', str(exc))}
        except (EngineTimeout, RecursionError):
            res = {'classification': 'undetermined', 'verdict': 'undetermined', 'method': 'timeout',
                   'limitations': ['the step check timed out']}
        except Exception as exc:
            res = {'classification': 'undetermined', 'verdict': 'undetermined', 'method': 'error',
                   'limitations': ['internal error: {}'.format(str(exc).split('\n')[0][:200])]}
        res['field'] = field
        return {'ok': True, 'result': self.richOut(res, opts)}

    def eqDomain(self, sides, v, field, extra=()):
        """Conditions of an equation (both sides) and the set of v where they hold."""
        conds, limits = [c for c in extra], []
        for t in sides:
            e1, info = self.resolveInert(t, field)
            cs, lim = self.condsOf(e1, field)
            limits += lim + info['limits']
            for c in info['conds'] + cs:
                for u in self.normCond(c, field):
                    if not any(u == o for o in conds):
                        conds.append(u)
        D = self.fieldSet(v, field)
        for c in conds:
            if v not in c.free_symbols:
                continue
            S = self.condSet(c, v, field)
            if S is None:
                limits.append('could not solve the domain condition {}'.format(self.mText(c)))
                continue
            D = sp.Intersection(D, S)
        return conds, D, limits

    def solveIn(self, l, r, v, field, extra=()):
        conds, D, limits = self.eqDomain([l, r], v, field, extra)
        S = sp.solveset(sp.Eq(l, r), v, self.fieldSet(v, field))
        if not S.has(sp.ConditionSet):
            S = sp.Intersection(S, D)
        return S, conds, limits

    def stepCause(self, lb, rb, la, ra, v, kind):
        dB, dA = lb - rb, la - ra
        if dA == 0 or dB == 0:
            return None
        try:
            q = sp.cancel(sp.together((lb ** 2 - rb ** 2) / dA))
            if kind in ('implies', 'not_equivalent') and q.is_number and q != 0:
                return 'the new equation is the old one squared (lhs^2 = rhs^2); squaring adds the solutions of ' \
                       'lhs = -rhs, so those must be rejected by checking them in the original equation'
            q = sp.cancel(sp.together(dB / dA))
            if q.is_number:
                return None
            if not q.is_rational_function(v):
                return None
            if kind == 'implied_by' and q.has(v):
                return 'the step divides by {}, which vanishes at solutions of the original equation; those ' \
                       'solutions are lost'.format(self.mText(q))
            if kind == 'implies' and (1 / q).has(v):
                return 'the step multiplies by {}, whose zeros become new (spurious) solutions'.format(self.mText(1 / q))
        except (ZeroDivisionError, TypeError, ValueError, NotImplementedError, sp.PolynomialError):
            return None
        return None

    def checkStep(self, B, A, V, field, raws=(None, None)):
        if isinstance(B, Seq) and len(B) == 1:
            B = B[0]
        if isinstance(A, Seq) and len(A) == 1:
            A = A[0]
        if not isRel(B) and not isRel(A):
            res = self.verifyRich(B, A, field, rawL=raws[0], rawR=raws[1])
            res['step'] = 'rewrite'
            res['classification'] = {'equal': 'equivalent', 'not_equal': 'not_equivalent'}.get(res['verdict'],
                                                                                            'undetermined')
            return res
        sb, sa = eqSides(B), eqSides(A)
        if sb is None or sa is None:
            raise MapleError('only equations (or expressions meaning expr = 0) can be compared as a step')
        rcB, rcA = self.rawConds(raws[0], field), self.rawConds(raws[1], field)
        rep = self.fieldRep(list(sb) + list(sa) + rcB + rcA, field)
        lb, rb = [t.xreplace(rep) for t in sb]
        la, ra = [t.xreplace(rep) for t in sa]
        rcB, rcA = [c.xreplace(rep) for c in rcB], [c.xreplace(rep) for c in rcA]
        syms = sorted({s for t in [lb, rb, la, ra] + rcB + rcA for s in t.free_symbols if isSym(s)}, key=lambda s: s.name)
        if V is not None:
            if not isSym(V):
                raise MapleError('var must be a name')
            v = next((s for s in syms if s.name == V.name), V)
        elif len(syms) == 1:
            v = syms[0]
        else:
            raise MapleError('give var: the equations contain {}'.format(', '.join(s.name for s in syms) or 'no unknown'))
        params = [s for s in syms if s != v]
        res = {'step': 'equation', 'var': v.name, 'notes': [], 'limitations': [], 'method': 'solveset'}
        ok, got = timeLimited(25, lambda: (self.solveIn(lb, rb, v, field, rcB), self.solveIn(la, ra, v, field, rcA)))
        if not ok:
            res.update({'classification': 'undetermined', 'verdict': 'undetermined'})
            res['limitations'].append('solving the equations timed out')
            return res
        (Sb, cb, limB), (Sa, ca, limA) = got
        res['limitations'] += limB + limA
        conds = cb + [c for c in ca if not any(c == o for o in cb)]
        res['conditions'] = [t for c in conds for t in self.condTexts(c, field)[0]]
        whole = 'all real numbers' if field == 'real' else 'all complex numbers'
        res['domain'] = {'before': ' and '.join(t for c in self.dropImplied(cb, v, field)
                                                for t in self.condTexts(c, field)[0]) or whole,
                         'after': ' and '.join(t for c in self.dropImplied(ca, v, field)
                                               for t in self.condTexts(c, field)[0]) or whole}
        okT, texts = timeLimited(8, lambda: (self.setText(Sb, v), self.setText(Sa, v)))
        if okT:
            (tb, kb), (ta, ka) = texts
            res['solutions_before'], res['solutions_after'] = tb, ta
            res['family'] = 'family' in (kb, ka)
        for name, S in (('original', Sb), ('new', Sa)):
            if S.has(sp.ConditionSet):
                res.update({'classification': 'undetermined', 'verdict': 'undetermined'})
                res['notes'].append('solveset could not solve the {} equation completely (it returned a '
                                    'ConditionSet), so the solution sets cannot be compared'.format(name))
                return res
        okC, sets = timeLimited(10, lambda: (sp.Complement(Sa, Sb), sp.Complement(Sb, Sa)))
        if not okC:
            res.update({'classification': 'undetermined', 'verdict': 'undetermined'})
            res['limitations'].append('comparing the solution sets timed out')
            return res
        extra, lost = sets
        ne, nl = nonEmpty(extra), nonEmpty(lost)
        if ne is None or nl is None:
            res.update({'classification': 'undetermined', 'verdict': 'undetermined'})
            res['notes'].append('could not decide whether the two solution sets are equal')
            return res
        kind = {(False, False): 'equivalent', (True, False): 'implies', (False, True): 'implied_by',
                (True, True): 'not_equivalent'}[(ne, nl)]
        if params:
            okP, q = timeLimited(5, lambda: sp.cancel(sp.together((lb - rb) / (la - ra))))
            if okP and q is not None and not q.is_number and not q.has(v):
                zq = sp.solve(q, params) if q.free_symbols else []
                zi = sp.solve(1 / q, params) if q.free_symbols else []
                if zq and kind in ('equivalent', 'implies'):
                    kind = 'implied_by' if kind == 'equivalent' else 'not_equivalent'
                    res['notes'].append('the step divides by {}: where {} = 0 every {} solves the original equation, '
                                        'but not the new one'.format(self.mText(q), self.mText(q), v.name))
                if zi and kind in ('equivalent', 'implied_by'):
                    kind = 'implies' if kind == 'equivalent' else 'not_equivalent'
                    res['notes'].append('the step multiplies by {}, which can vanish'.format(self.mText(1 / q)))
            res['limitations'].append('solution sets were computed for generic values of {} (solveset treats '
                                      'parameters as generic)'.format(', '.join(s.name for s in params)))
        res['classification'] = kind
        res['verdict'] = 'equal' if kind == 'equivalent' else 'not_equal'
        if ne:
            res['spurious'] = self.setText(extra, v)[0]
        if nl:
            res['lost'] = self.setText(lost, v)[0]
        okW, cause = timeLimited(5, self.stepCause, lb, rb, la, ra, v, kind)
        if okW and cause:
            res['notes'].append(cause)
        return res

    # ---- check_solution ----

    def opCheckSolution(self, req):
        opts = req.get('opts') or {}
        self.setup(opts)
        field = self.fieldOpt(opts)
        try:
            eqs = [self.cv.toSympy(e) for e in req['equations']]
            cands = [[self.cv.toSympy(e) for e in c] for c in req['candidates']]
        except (MapleError, KeyError, TypeError, ValueError, IndexError) as exc:
            return {'ok': False, 'error': self.errMsg('check_solution', str(exc))}
        try:
            raws = req['equations']
            er = req.get('equations_raw')
            if isinstance(er, list) and len(er) == len(raws):
                raws = [[a, b] for a, b in zip(raws, er)]
            res = self.checkSolution(eqs, cands, field, raws)
        except MapleError as exc:
            return {'ok': False, 'error': self.errMsg('check_solution', str(exc))}
        except (EngineTimeout, RecursionError):
            res = {'verdict': 'undetermined', 'method': 'timeout', 'limitations': ['the check timed out']}
        except Exception as exc:
            res = {'verdict': 'undetermined', 'method': 'error',
                   'limitations': ['internal error: {}'.format(str(exc).split('\n')[0][:200])]}
        res['field'] = field
        return {'ok': True, 'result': self.richOut(res, opts)}

    def checkOne(self, l, r, pt, field, conds):
        """One equation at one candidate point: (status, detail)."""
        for c in conds:
            try:
                tv = self.truthAt(c.subs(pt), {})
            except (TypeError, ValueError, ZeroDivisionError):
                tv = None
            if tv is False:
                return 'outside_domain', 'the condition {} of the original equation fails'.format(self.mText(c))
        try:
            lv, rv = l.subs(pt), r.subs(pt)
        except (TypeError, ValueError, ZeroDivisionError):
            return 'undetermined', 'could not substitute'
        if any(x.has(sp.nan, sp.zoo) for x in (lv, rv)):
            return 'outside_domain', 'the equation is not defined there'
        z, best = self.symbolicZero(lv - rv)
        if z:
            return 'satisfied', 'lhs - rhs simplifies to 0'
        a, b = self.evalAt(lv, {}), self.evalAt(rv, {})
        if a is None or b is None:
            return 'undetermined', 'could not evaluate lhs - rhs = {}'.format(self.mText(best))
        if field == 'real' and not (self.isReal(a) and self.isReal(b)):
            return 'outside_domain', 'a side is not real there (lhs = {}, rhs = {})'.format(fmtValue(a), fmtValue(b))
        if abs(a - b) > 1e-20 * max(1, abs(a), abs(b)):
            return 'not_satisfied', 'lhs = {}, rhs = {}'.format(fmtValue(a), fmtValue(b))
        return 'satisfied_numeric', 'lhs - rhs vanishes numerically (40 digits) but was not simplified to 0'

    def checkSolution(self, eqs, cands, field, raws):
        pairs = []
        for e in eqs:
            s = eqSides(e)
            if s is None:
                raise MapleError('equations must be equations or expressions (meaning expr = 0)')
            pairs.append(s)
        allE = [t for p in pairs for t in p] + [t for c in cands for t in c]
        rep = self.fieldRep([t for t in allE if isinstance(t, sp.Basic)], field)
        pairs = [(l.xreplace(rep), r.xreplace(rep)) for l, r in pairs]
        byName = {s.name: s for s in rep.values()}
        byName.update({s.name: s for p in pairs for t in p for s in t.free_symbols if isSym(s)})
        res = {'notes': [], 'limitations': [], 'method': 'symbolic', 'candidates': []}
        eqConds, texts = [], []
        for (l, r), raw in zip(pairs, raws):
            cs = [c.xreplace(rep) for c in self.rawConds(raw, field)]
            for t in (l, r):
                e1, info = self.resolveInert(t, field)
                cs += info['conds'] + self.condsOf(e1, field)[0]
            cs = [u for c in cs for u in self.normCond(c, field)]
            eqConds.append(cs)
            for c in cs:
                for t in self.condTexts(c, field)[0]:
                    if t not in texts:
                        texts.append(t)
        res['conditions'] = texts
        variables = []
        accepted = []
        for cand in cands:
            pt = {}
            for c in cand:
                if not isRel(c) or relParts(c)[0] != 'eq':
                    raise MapleError('solution must be equations like x = 2')
                _, k, val = relParts(c)
                if isSym(k):
                    k = byName.get(k.name, k)
                    if k not in variables:
                        variables.append(k)
                pt[k] = val.xreplace(rep) if isinstance(val, sp.Basic) else val
            status, details = 'accepted', []
            for i, (l, r) in enumerate(pairs):
                st, det = self.checkOne(l, r, pt, field, eqConds[i])
                details.append('equation {}: {}: {}'.format(i + 1, st.replace('_', ' '), det))
                if st in ('outside_domain', 'not_satisfied'):
                    status = 'rejected'
                elif st == 'undetermined' and status == 'accepted':
                    status = 'undetermined'
                elif st == 'satisfied_numeric' and status == 'accepted':
                    res['method'] = 'numeric'
            ptText = ', '.join('{} = {}'.format(self.mText(k), self.mText(v)) for k, v in pt.items())
            res['candidates'].append({'point': ptText, 'status': status, 'details': details})
            if status == 'accepted':
                accepted.append(pt)
        sts = [c['status'] for c in res['candidates']]
        res['verdict'] = 'not_equal' if 'rejected' in sts else ('equal' if sts and all(s == 'accepted' for s in sts)
                                                                else 'undetermined')
        rejected = [c['point'] for c in res['candidates'] if c['status'] == 'rejected']
        if rejected:
            res['rejected'] = rejected
        self.completeness(pairs, variables, accepted, field, res, eqConds)
        return res

    def completeness(self, pairs, variables, accepted, field, res, eqConds):
        """Are the accepted candidates all the solutions? Says so only when solveset/solve can tell."""
        numeric = all(isinstance(v, sp.Basic) and v.is_number for pt in accepted for v in pt.values())
        if len(pairs) == 1 and len(variables) == 1 and numeric:
            v = variables[0]
            l, r = pairs[0]
            if any(s != v for s in (l - r).free_symbols):
                res['complete'] = None
                res['completeness'] = 'not established: the equation has parameters besides {}'.format(v.name)
                return
            ok, got = timeLimited(15, self.solveIn, l, r, v, field, eqConds[0])
            if not ok:
                res['complete'] = None
                res['completeness'] = 'not established: solving the equation timed out'
                return
            S, _, _ = got
            txt, kind = self.setText(S, v) if not S.has(sp.ConditionSet) else (self.mText(S), 'unsolved')
            if kind != 'unsolved':
                res['all_solutions'] = txt
            if kind == 'unsolved' or S.has(sp.ConditionSet):
                res['complete'] = None
                res['completeness'] = 'not established: solveset could not solve the equation (ConditionSet)'
            elif kind in ('family', 'all', 'set'):
                res['complete'] = False
                res['family'] = kind == 'family'
                res['completeness'] = 'the equation has infinitely many solutions over the {}s ({}); a finite ' \
                                      'list of candidates cannot be complete'.format(field, txt)
            elif kind == 'empty':
                res['complete'] = not accepted
                res['completeness'] = 'the equation has no solutions over the {}s'.format(field)
            elif isinstance(S, sp.FiniteSet):
                got = [pt[v] for pt in accepted if v in pt]
                missing = [p for p in S if not any(sp.simplify(p - g) == 0 for g in got)]
                res['complete'] = not missing
                res['completeness'] = 'the accepted candidates are all the solutions over the {}s'.format(field) \
                    if not missing else 'missing solutions: {}'.format(
                        ', '.join('{} = {}'.format(v.name, self.mText(p)) for p in missing))
            else:
                res['complete'] = None
                res['completeness'] = 'not established'
            return
        polys = len(variables) >= 1 and all((l - r).is_polynomial(*variables) for l, r in pairs) and numeric
        if polys and all((l - r).free_symbols <= set(variables) for l, r in pairs):
            ok, sols = timeLimited(10, lambda: sp.solve([l - r for l, r in pairs], variables, dict=True))
            if ok and isinstance(sols, list) and all(set(s.keys()) == set(variables) for s in sols):
                if field == 'real':
                    sols = [s for s in sols if all(x.is_real for x in s.values())]
                missing = [s for s in sols if not any(all(sp.simplify(s[k] - pt.get(k, sp.nan)) == 0
                                                          for k in variables) for pt in accepted)]
                res['complete'] = not missing
                res['completeness'] = 'the accepted candidates are all the solutions of the polynomial system' \
                    if not missing else 'missing solutions: {}'.format('; '.join(
                        ', '.join('{} = {}'.format(k.name, self.mText(s[k])) for k in variables) for s in missing))
                return
        res['complete'] = None
        res['completeness'] = 'not established: only the given candidates were checked'


# ----------------------------------------------------------------------------
# latex and pretty
# ----------------------------------------------------------------------------

class PrintMixin(object):
    def opLatex(self, req):
        self.setup(req.get('opts'))
        try:
            e = self.cv.toSympy(req['expr'])
            pr = MapleLatex({'inv_trig_style': 'full', 'ln_notation': True})
            return {'ok': True, 'result': pr.doprint(e)}
        except MapleError as exc:
            return {'ok': False, 'error': self.errMsg('latex', str(exc))}
        except Exception as exc:
            return {'ok': False, 'error': self.errMsg('latex', self.pyMessage(exc))}

    def opPretty(self, req):
        self.setup(req.get('opts'))
        width = req.get('width') or 80
        try:
            width = max(20, int(width))
        except (TypeError, ValueError):
            width = 80
        try:
            e = self.cv.toSympy(req['expr'])
            pr = MaplePretty({'use_unicode': True, 'wrap_line': True, 'num_columns': width})
            return {'ok': True, 'result': pr.doprint(e)}
        except MapleError as exc:
            return {'ok': False, 'error': self.errMsg('pretty', str(exc))}
        except Exception as exc:
            return {'ok': False, 'error': self.errMsg('pretty', self.pyMessage(exc))}


# ----------------------------------------------------------------------------
# Calculus
# ----------------------------------------------------------------------------

def specTuple(spec):
    v, lo, hi = splitSpec(spec)
    if lo is not None and hi is not None:
        return (v, lo, hi)
    if lo is not None:
        return (v, lo)
    return v


def inertInt(f, *specs):
    return sp.Integral(f, *[specTuple(s) for s in specs])


def inertSum(f, spec):
    v, lo, hi = splitSpec(spec)
    return sp.Sum(f, (v, lo, hi))


def inertProduct(f, spec):
    v, lo, hi = splitSpec(spec)
    return sp.Product(f, (v, lo, hi))


def inertLimit(f, spec, *rest):
    v, pt, _ = splitSpec(spec)
    if rest:
        d = optName(rest[0])
        return sp.Limit(f, v, pt, '+' if d == 'right' else ('-' if d == 'left' else '+-'))
    return sp.Limit(f, v, pt, '+-')


def inertDiff(f, *vs):
    flat = []
    for v in vs:
        flat.append(v)
    return sp.Derivative(f, *flat)


INERT.update({'Int': inertInt, 'Sum': inertSum, 'Product': inertProduct, 'Limit': inertLimit,
              'Diff': inertDiff})


def etaReduce(lam):
    """x -> g(x) becomes the bare name g (Maple prints D(sin) as cos)."""
    if isinstance(lam, sp.Lambda) and len(lam.variables) == 1:
        body = lam.expr
        if isinstance(body, sp.Function) and len(body.args) == 1 and body.args[0] == lam.variables[0] \
                and type(body) in REV and not isinstance(body, sp.Pow):
            return sp.Symbol(REV[type(body)])
    return lam


class CalculusMixin(object):
    """diff, D, int, limit, series, sum ... with Maple conventions."""

    def unevaluated(self, name, a):
        return Raw({'fn': name, 'args': [self.cv.toExpr(x) for x in a]})

    def doDiff(self, a):
        needArgs(a, 2)
        return diffFn(a[0], *a[1:])

    def doD(self, a):
        needArgs(a, 1)
        f = a[0]
        idx = [intOf(i) for i in self.callIdx]
        if isinstance(f, sp.Lambda):
            vs = f.variables
            body = f.expr
            if idx:
                for i in idx:
                    body = sp.diff(body, vs[i - 1])
            else:
                if len(vs) != 1:
                    raise MapleError('unable to determine the differentiation variable')
                body = sp.diff(body, (vs[0], self.callOrder))
            return etaReduce(sp.Lambda(vs, body))
        if isSym(f) and f.name in FUNCS:
            t = sp.Dummy('x')
            return etaReduce(sp.Lambda((t,), sp.diff(FUNCS[f.name](t), (t, self.callOrder))))
        if isinstance(f, sp.Basic) and idx == []:
            return self.unevaluated('D', [f])
        raise MapleError('invalid arguments to D')

    def postIndef(self, r):
        """Maple returns the generic antiderivative, not SymPy's piecewise special cases."""
        if isinstance(r, sp.Piecewise) and len(r.args) >= 2:
            e0, c0 = r.args[0]
            if isinstance(c0, sp.core.relational.Ne) or isinstance(c0, sp.Not):
                return e0
        return r

    def doInt(self, a):
        needArgs(a, 2)
        f = a[0]
        specs = a[1:]
        tuples = [specTuple(s) for s in specs]
        definite = all(isinstance(t, tuple) and len(t) == 3 for t in tuples)

        def go(fe):
            if not isinstance(fe, sp.Basic):
                return fe
            if definite:
                r = sp.integrate(fe, *tuples, conds='none')
            else:
                r = self.postIndef(sp.integrate(fe, *tuples))
            if isinstance(r, sp.Basic) and (r.has(sp.Integral)):
                return None
            if r is sp.nan:
                return sp.nan
            return r
        if isinstance(f, (MList, MSet, Mat)):
            out = mapS(lambda e: go(e) if go(e) is not None else e, f)
            return out
        r = go(f)
        if r is None:
            return self.unevaluated('int', a)
        return r

    def doInert(self, a):
        return INERT[self.curName](*a)

    def doValue(self, a):
        needArgs(a, 1)

        def go(e):
            if not isinstance(e, sp.Basic):
                return e
            return e.doit(conds='none')
        return mapS(go, a[0])

    def limitOne(self, f, v, pt, dirn):
        if pt in (sp.oo, -sp.oo):
            r = sp.limit(f, v, pt)
        elif dirn == 'right':
            r = sp.limit(f, v, pt, '+')
        elif dirn == 'left':
            r = sp.limit(f, v, pt, '-')
        else:
            r1 = sp.limit(f, v, pt, '+')
            r2 = sp.limit(f, v, pt, '-')
            if r1 == r2:
                r = r1
            else:
                same = False
                try:
                    if r1.is_finite and r2.is_finite:
                        same = sp.simplify(r1 - r2) == 0
                except (TypeError, ValueError, AttributeError):
                    same = False
                r = r1 if same else sp.nan
        return r

    def doLimit(self, a):
        needArgs(a, 2, 3)
        f = a[0]
        v, pt, _ = splitSpec(a[1])
        if pt is None:
            raise MapleError('invalid limit specification')
        dirn = optName(a[2]) if len(a) > 2 else None

        def go(fe):
            if not isinstance(fe, sp.Basic):
                return fe
            r = self.limitOne(fe, v, pt, dirn)
            if isinstance(r, sp.Limit) or (isinstance(r, sp.Basic) and r.has(sp.Limit)):
                return None
            return r
        r = go(f) if not isinstance(f, (MList, MSet, Mat)) else mapS(go, f)
        if r is None:
            return self.unevaluated('limit', a)
        return r

    def seriesOf(self, f, spec, n, name):
        v, pt, _ = splitSpec(spec) if isRel(spec) else (spec, None, None)
        if pt is None:
            pt = sp.Integer(0)
        if not isSym(v):
            raise MapleError('invalid series variable')
        try:
            r = f.series(v, pt, n)
        except NotImplementedError:
            raise MapleError('unable to compute series')
        except ValueError:
            raise MapleError('unable to compute series')
        return r, v, pt

    def doSeries(self, a):
        needArgs(a, 2, 3)
        n = intOf(a[2]) if len(a) > 2 else 6
        if n < 1:
            raise MapleError('invalid order')

        def go(f):
            if not isinstance(f, sp.Basic):
                return f
            return self.seriesOf(f, a[1], n, 'series')[0]
        return mapS(go, a[0])

    def doTaylor(self, a):
        needArgs(a, 2, 3)
        n = intOf(a[2]) if len(a) > 2 else 6

        def go(f):
            r, v, pt = self.seriesOf(f, a[1], n, 'taylor')
            body = r.removeO() if r.has(sp.Order) else r
            for t in sp.Add.make_args(sp.expand(body.subs(v, v + pt) if pt not in (sp.oo, -sp.oo) else body)):
                c, rest = t.as_independent(v)
                p = rest.as_base_exp()[1] if rest.is_Pow else sp.Integer(1 if rest == v else 0)
                if p.is_number and p.is_negative:
                    raise MapleError('does not have a taylor expansion, try series()')
            return r
        return mapS(lambda f: go(f) if isinstance(f, sp.Basic) else f, a[0])

    def doMtaylor(self, a):
        needArgs(a, 2, 3)
        f = a[0]
        n = intOf(a[2]) if len(a) > 2 else 6
        spec = a[1]
        items = list(spec) if isinstance(spec, (MList, MSet)) else [spec]
        vs, pts = [], []
        for it in items:
            v, pt, _ = splitSpec(it) if isRel(it) else (it, None, None)
            vs.append(v)
            pts.append(pt if pt is not None else sp.Integer(0))
        t = sp.Dummy('t')
        sub = {v: p + t * (v - p) for v, p in zip(vs, pts)}
        g = f.subs(sub, simultaneous=True)
        ser = sp.series(g, t, 0, n).removeO()
        return sp.expand(ser.subs(t, 1))

    def doSum(self, a):
        needArgs(a, 2)
        f, spec = a[0], a[1]
        v, lo, hi = splitSpec(spec)
        if not isSym(v):
            raise MapleError('invalid summation variable')

        def go(fe):
            if lo is None:
                j = sp.Dummy('j')
                r = sp.summation(fe.subs(v, j), (j, 0, v - 1))
            else:
                r = sp.summation(fe, (v, lo, hi))
                if isinstance(r, sp.Piecewise) and r.args[-1][0].has(sp.Sum) and not r.args[0][0].has(sp.Sum):
                    r = r.args[0][0]
            if r.has(sp.Sum):
                return None
            return r
        r = go(f) if not isinstance(f, (MList, MSet, Mat)) else mapS(lambda e: go(e) or e, f)
        if r is None:
            return self.unevaluated('sum', a)
        return r

    def doProduct(self, a):
        needArgs(a, 2)
        f, spec = a[0], a[1]
        v, lo, hi = splitSpec(spec)
        if lo is None:
            raise MapleError('invalid product range')
        r = sp.product(f, (v, lo, hi))
        if r.has(sp.Product):
            return self.unevaluated('product', a)
        return r

    def doResidue(self, a):
        needArgs(a, 2)
        v, pt, _ = splitSpec(a[1])
        return sp.residue(a[0], v, pt)

    def doSingular(self, a):
        needArgs(a, 1, 2)
        f = a[0]
        v = theVar(f, a[1] if len(a) > 1 and isSym(a[1]) else None)
        s = sp.singularities(f, v)
        if isinstance(s, sp.FiniteSet) or isinstance(s, (set, frozenset)):
            return mkSet([mkEq(v, p) for p in s])
        raise MapleError('unable to determine singularities')

    def doImplicitdiff(self, a):
        needArgs(a, 3)
        F, y = a[0], a[1]
        xs = a[2:]
        if isRel(F):
            F = asExpr(F)
        yv = y
        n = len(xs)
        x = xs[0]
        for xx in xs[1:]:
            if xx != x:
                raise MapleError('only one independent variable is supported')
        if isinstance(yv, sp.core.function.AppliedUndef):
            fsym = sp.Dummy('yy')
            Fs = F.subs(yv, fsym)
            return sp.idiff(Fs, fsym, x, n)
        return sp.idiff(F, yv, x, n)

    def doExtrema(self, a):
        needArgs(a, 3, 4)
        f, cons, vs = a[0], a[1], a[2]
        vs = list(vs) if isinstance(vs, (MSet, MList)) else [vs]
        cs = []
        for c in (list(cons) if isinstance(cons, (MSet, MList)) else [cons]):
            cs.append(asExpr(c))
        lams = [sp.Dummy('lam') for _ in cs]
        L = f - sum(l * c for l, c in zip(lams, cs))
        eqs = [sp.diff(L, v) for v in vs] + cs
        sols = sp.solve(eqs, vs + lams, dict=True)
        vals = []
        for s in sols:
            if all(v in s for v in vs):
                val = sp.simplify(f.subs({v: s[v] for v in vs}))
                if not any(sameObj(val, o) for o in vals):
                    vals.append(val)
        return mkSet(vals)

    def maxmin(self, a, isMax, name):
        from sympy.calculus.util import maximum, minimum
        needArgs(a, 2)
        f = a[0]
        v, lo, hi = splitSpec(a[1])
        if not isSym(v):
            raise MapleError('unable to compute')
        vr = sp.Dummy(v.name, real=True)
        fr = f.xreplace({v: vr})
        dom = sp.Interval(lo, hi) if hi is not None else sp.S.Reals
        fn = maximum if isMax else minimum
        r = fn(fr, vr, dom)
        return r

    def doMaximize(self, a):
        return self.maxmin(a, True, 'maximize')

    def doMinimize(self, a):
        return self.maxmin(a, False, 'minimize')


HANDLERS.update({
    'diff': 'doDiff', 'D': 'doD', 'int': 'doInt', 'value': 'doValue', 'limit': 'doLimit',
    'series': 'doSeries', 'taylor': 'doTaylor', 'mtaylor': 'doMtaylor', 'sum': 'doSum',
    'product': 'doProduct', 'residue': 'doResidue', 'singular': 'doSingular',
    'implicitdiff': 'doImplicitdiff', 'extrema': 'doExtrema', 'maximize': 'doMaximize',
    'minimize': 'doMinimize',
})
# diff and Diff inert/active forms share the same table row names
HANDLERS.update({'Int': 'doInert', 'Sum': 'doInert', 'Product': 'doInert', 'Limit': 'doInert', 'Diff': 'doInert'})


# ----------------------------------------------------------------------------
# solve, fsolve, isolve, rsolve, dsolve, pdsolve, odetest
# ----------------------------------------------------------------------------

SOLVE_OPTS = {'useassumptions', 'AllSolutions', 'explicit', 'parametric', 'dimensionless', 'implicit',
              'allsolutions', 'real', 'complex', 'avoid', 'maxsols', 'method', 'ignoreanalytic'}


def solKey(v):
    """Order for solutions: real values descending, then complex with positive imaginary part first."""
    try:
        c = complex(v.evalf(20))
        return (0 if abs(c.imag) < 1e-12 else 1, -c.real, -c.imag)
    except (TypeError, ValueError, AttributeError):
        return (2, 0, 0)


def orderSolutions(vals):
    try:
        if vals and all(isinstance(v, sp.CRootOf) for v in vals):
            return sorted(vals, key=lambda v: (str(v.poly), v.index))
        return sorted(vals, key=solKey)
    except (TypeError, ValueError):
        return list(vals)


class SolveMixin(object):
    def isOpt(self, o):
        n = optName(o)
        if n in SOLVE_OPTS:
            return True
        if isRel(o):
            op, l, r = relParts(o)
            return op == 'eq' and optName(l) in SOLVE_OPTS
        return False

    def splitEqs(self, eqsArg):
        eqs = list(eqsArg) if isinstance(eqsArg, (MSet, MList, Seq)) else [eqsArg]
        exprs, ineqs = [], []
        for q in eqs:
            if isRel(q):
                op, l, r = relParts(q)
                if op == 'eq':
                    exprs.append(l - r)
                elif op in ('lt', 'le'):
                    ineqs.append(q)
                else:
                    ineqs.append(q)
            elif q is sp.false:
                exprs.append(sp.Integer(1))
            elif q is sp.true:
                continue
            else:
                exprs.append(q)
        return exprs, ineqs

    def doSolve(self, a):
        needArgs(a, 1)
        rest = [o for o in a[1:] if not self.isOpt(o)]
        eqsArg = a[0]
        if isinstance(eqsArg, (Mat,)):
            raise MapleError('wrong number (or type) of arguments')
        exprs, ineqs = self.splitEqs(eqsArg)
        varsArg = rest[0] if rest else None
        mode = 'values'
        if varsArg is None:
            syms = set()
            for e in exprs + [r for q in ineqs for r in q.args]:
                syms |= {s for s in e.free_symbols if isSym(s)}
            vs = sorted(syms, key=lambda s: s.name)
            mode = 'values' if len(vs) <= 1 else 'sets'
            if not vs:
                vs = [sp.Symbol('x')]
        elif isinstance(varsArg, MSet):
            vs, mode = list(varsArg), 'sets'
        elif isinstance(varsArg, MList):
            vs, mode = list(varsArg), 'lists'
        else:
            vs, mode = [varsArg], 'values'
        for v in vs:
            if not (isSym(v) or isinstance(v, (sp.core.function.AppliedUndef, sp.Derivative))):
                raise MapleError('wrong number (or type) of arguments')
        if ineqs:
            return self.solveIneq(exprs, ineqs, vs, mode)
        if all(e == 0 for e in exprs):
            return self.shapeSolutions([{v: v for v in vs}], vs, mode)
        numeric = any(e.has(sp.Float) for e in exprs)
        if numeric:
            exprs = [exactFloats(e)[0] for e in exprs]
        try:
            sols = sp.solve(exprs if len(exprs) > 1 else exprs[0], vs, dict=True)
        except (NotImplementedError, ValueError, TypeError, AttributeError):
            sols = self.solveOverReals(exprs, vs)
            if sols is None:
                return self.unevaluated('solve', a)
        if numeric:
            self.cv.floatDigits = self.digits
            self.cv.keepZeros = True
            sols = [{k: (val.evalf(self.digits + 5) if isinstance(val, sp.Basic) else val) for k, val in s.items()}
                    for s in sols]
        return self.shapeSolutions(sols, vs, mode)

    def solveOverReals(self, exprs, vs):
        """Second attempt for equations such as abs(x-1)=2, which SymPy only solves for real unknowns."""
        fwd = {v: sp.Dummy(v.name, real=True) for v in vs if isSym(v)}
        if not fwd:
            return None
        back = {d: v for v, d in fwd.items()}
        try:
            eqs = [e.xreplace(fwd) for e in exprs]
            sols = sp.solve(eqs if len(eqs) > 1 else eqs[0], list(fwd.values()), dict=True)
        except (NotImplementedError, ValueError, TypeError, AttributeError):
            return None
        out = []
        for s in sols:
            out.append({back.get(k, k): (val.xreplace(back) if isinstance(val, sp.Basic) else val) for k, val in s.items()})
        return out

    def shapeSolutions(self, sols, vs, mode):
        if mode == 'values':
            v = vs[0]
            vals = []
            for s in sols:
                if v in s:
                    if not any(sameObj(s[v], o) for o in vals):
                        vals.append(s[v])
            return Seq(orderSolutions(vals))
        out = []
        for s in sols:
            eqs = [mkEq(v, s.get(v, v)) for v in vs]
            out.append(eqs)
        out.sort(key=lambda es: [solKey(e.rhs) if isinstance(e, sp.Basic) else (3, 0, 0) for e in es])
        if mode == 'sets':
            return Seq([mkSet(es) for es in out])
        return MList([MList(es) for es in out]) if out else Seq()

    def solveIneq(self, exprs, ineqs, vs, mode):
        if len(vs) != 1 or exprs:
            raise MapleError('only a single inequality in one variable is supported')
        v = vs[0]
        vr = sp.Dummy(v.name, real=True)
        rels = [self.toSympyRel(q).xreplace({v: vr}) for q in ineqs]
        rel = rels[0] if len(rels) == 1 else sp.And(*rels)
        try:
            res = sp.solve_univariate_inequality(rel, vr, relational=False) if len(rels) == 1 \
                else sp.solveset(rel, vr, domain=sp.S.Reals)
        except (NotImplementedError, ValueError, TypeError):
            raise MapleError('unable to solve the inequality')
        pieces = res.args if isinstance(res, sp.Union) else [res]
        out = []
        for p in pieces:
            if p is sp.S.EmptySet:
                continue
            if isinstance(p, sp.Interval):
                if mode == 'values':
                    lo = p.start if not p.left_open or p.start == -sp.oo else sp.Function('Open')(p.start)
                    hi = p.end if not p.right_open or p.end == sp.oo else sp.Function('Open')(p.end)
                    out.append(sp.Function('RealRange')(lo, hi))
                else:
                    rs = []
                    if p.start != -sp.oo:
                        rs.append(mkRel('lt' if p.left_open else 'le', p.start, v))
                    if p.end != sp.oo:
                        rs.append(mkRel('lt' if p.right_open else 'le', v, p.end))
                    out.append(mkSet(rs) if rs else mkSet([mkEq(v, v)]))
            elif isinstance(p, sp.FiniteSet):
                for q in p.args:
                    out.append(q if mode == 'values' else mkSet([mkEq(v, q)]))
            elif p is sp.S.Reals:
                out.append(sp.Function('RealRange')(-sp.oo, sp.oo) if mode == 'values' else mkSet([mkEq(v, v)]))
            else:
                raise MapleError('unable to solve the inequality')
        if mode == 'lists':
            return MList(out)
        return Seq(out)

    def toSympyRel(self, q):
        if isinstance(q, RelObj):
            raise MapleError('invalid relation')
        op, l, r = relParts(q)
        if op == 'lt':
            return sp.Lt(l, r)
        if op == 'le':
            return sp.Le(l, r)
        if op == 'neq':
            return sp.Ne(l, r)
        return sp.Eq(l, r)

    # ---- fsolve ----

    def doFsolve(self, a):
        needArgs(a, 1)
        opts = [optName(o) for o in a[1:] if optName(o)]
        specs = [o for o in a[1:] if not optName(o)]
        eqsArg = a[0]
        exprs, ineqs = self.splitEqs(eqsArg)
        if ineqs:
            raise MapleError('inequalities are not supported')
        self.cv.floatDigits = self.digits
        self.cv.keepZeros = True
        complexOpt = 'complex' in opts
        guess = {}
        vs = None
        for s in specs:
            if isinstance(s, (MSet, MList)):
                vs = []
                for it in s:
                    if isRel(it):
                        v, lo, hi = splitSpec(it)
                        guess[v] = (lo, hi)
                        vs.append(v)
                    else:
                        vs.append(it)
            elif isRel(s):
                v, lo, hi = splitSpec(s)
                guess[v] = (lo, hi)
                vs = [v]
            else:
                vs = [s]
        if vs is None:
            syms = set()
            for e in exprs:
                syms |= {x for x in e.free_symbols if isSym(x)}
            vs = sorted(syms, key=lambda s: s.name)
        if len(exprs) == 1 and len(vs) == 1 and not isinstance(eqsArg, MSet):
            res = self.fsolve1(exprs[0], vs[0], guess.get(vs[0]), complexOpt)
            if not res:
                return self.unevaluated('fsolve', a)
            return Seq(res)
        res = self.fsolveSystem(exprs, vs, guess)
        if res is None:
            return self.unevaluated('fsolve', a)
        return mkSet([mkEq(v, val) for v, val in zip(vs, res)])

    def toFloat(self, mp):
        return sp.Float(mp, self.digits + 5)

    def fsolve1(self, e, v, gspec, complexOpt):
        D = self.digits
        with mpmath.workdps(D + 20):
            lo = hi = None
            if gspec is not None:
                lo, hi = gspec
            isPoly = False
            try:
                poly = sp.Poly(e, v)
                isPoly = poly.degree() >= 1 and all(c.is_number for c in poly.coeffs())
            except (sp.PolynomialError, sp.GeneratorsNeeded):
                isPoly = False
            roots = []
            if isPoly:
                rts = poly.nroots(n=D + 10, maxsteps=500)
                for r in rts:
                    if complexOpt:
                        roots.append(r)
                    elif abs(sp.im(r)) < sp.Float(10) ** (-(D + 5)) * max(1, abs(r)):
                        roots.append(sp.re(r))
                if hi is not None:
                    roots = [r for r in roots if sp.im(r) == 0 and lo <= r <= hi]
                if not complexOpt:
                    roots = sorted(roots, key=lambda r: float(r))
                    uniq = []
                    for r in roots:
                        if not uniq or abs(r - uniq[-1]) > sp.Float(10) ** (-(D + 2)) * max(1, abs(r)):
                            uniq.append(r)
                    roots = uniq
                return [sp.Float(r, D + 5) if r.is_real else r.evalf(D + 5) for r in roots]
            try:
                f = sp.lambdify(v, e, 'mpmath')
            except (TypeError, ValueError):
                return []
            found = []
            if lo is not None and hi is not None and not (lo == hi):
                a, b = mpmath.mpf(str(sp.N(lo, 30))), mpmath.mpf(str(sp.N(hi, 30)))
                N = 400
                xs = [a + (b - a) * i / N for i in range(N + 1)]
                vals = []
                for xv in xs:
                    try:
                        vals.append(f(xv))
                    except (ZeroDivisionError, ValueError, OverflowError):
                        vals.append(None)
                for i in range(N):
                    f0, f1 = vals[i], vals[i + 1]
                    if f0 is None or f1 is None:
                        continue
                    if f0 == 0:
                        found.append(xs[i])
                    elif f0 * f1 < 0:
                        try:
                            r = mpmath.findroot(f, (xs[i], xs[i + 1]), solver='anderson', tol=mpmath.mpf(10) ** (-(D + 10)), maxsteps=200)
                            found.append(r)
                        except (ValueError, ZeroDivisionError, mpmath.libmp.NoConvergence):
                            continue
                out = []
                for r in found:
                    if not out or abs(r - out[-1]) > mpmath.mpf(10) ** (-(D))*max(1, abs(r)):
                        out.append(r)
                return [self.toFloat(r) for r in out]
            starts = []
            if lo is not None:
                starts = [mpmath.mpf(str(sp.N(lo, 30)))]
            else:
                starts = [mpmath.mpf(s) for s in ('0.1', '1', '-1', '2', '-2', '0.5', '-0.5', '3', '-3', '5', '-5', '10', '-10', '0.01')]
            for s0 in starts:
                try:
                    r = mpmath.findroot(f, s0, tol=mpmath.mpf(10) ** (-(D + 10)), maxsteps=100)
                except (ValueError, ZeroDivisionError, mpmath.libmp.NoConvergence, TypeError):
                    continue
                try:
                    if abs(f(r)) < mpmath.mpf(10) ** (-(D)) and mpmath.im(r) == 0 or abs(f(r)) < mpmath.mpf(10) ** (-D):
                        if complexOpt or mpmath.im(r) == 0:
                            return [self.toFloat(mpmath.re(r)) if mpmath.im(r) == 0 else sp.Float(mpmath.re(r), D + 5) + sp.I * sp.Float(mpmath.im(r), D + 5)]
                except (ValueError, ZeroDivisionError, TypeError):
                    continue
            return []

    def fsolveSystem(self, exprs, vs, guess):
        D = self.digits
        if len(exprs) != len(vs):
            return None
        with mpmath.workdps(D + 20):
            fs = [sp.lambdify(vs, e, 'mpmath') for e in exprs]

            def F(*xs):
                return [f(*xs) for f in fs]
            starts = []
            g0 = []
            hasGuess = False
            for v in vs:
                gs = guess.get(v)
                if gs is None:
                    g0.append(None)
                else:
                    hasGuess = True
                    lo, hi = gs
                    if hi is None:
                        g0.append(mpmath.mpf(str(sp.N(lo, 30))))
                    else:
                        g0.append((mpmath.mpf(str(sp.N(lo, 30))) + mpmath.mpf(str(sp.N(hi, 30)))) / 2)
            if hasGuess and all(g is not None for g in g0):
                starts.append(g0)
            rng = random.Random(7)
            starts.append([mpmath.mpf(1)] * len(vs))
            starts.append([mpmath.mpf(0)] * len(vs))
            for _ in range(30):
                starts.append([mpmath.mpf(rng.uniform(-3, 3)) for _ in vs])
            for st in starts:
                try:
                    r = mpmath.findroot(F, st, tol=mpmath.mpf(10) ** (-(D + 10)), maxsteps=100)
                except (ValueError, ZeroDivisionError, mpmath.libmp.NoConvergence, TypeError):
                    continue
                vals = [r[i] for i in range(len(vs))]
                if all(abs(x) < mpmath.mpf(10) ** (-D) for x in F(*vals)):
                    return [self.toFloat(mpmath.re(x)) for x in vals]
            return None

    def doIsolve(self, a):
        needArgs(a, 1, 2)
        exprs, ineqs = self.splitEqs(a[0])
        if len(a) > 1:
            vs = list(a[1]) if isinstance(a[1], (MSet, MList)) else [a[1]]
        else:
            vs = sorted({s for e in exprs for s in e.free_symbols if isSym(s)}, key=lambda s: s.name)
        if len(exprs) != 1:
            raise MapleError('only a single equation is supported')
        sols = sp.diophantine(exprs[0], syms=vs)
        out = []
        for tup in sols:
            eqs = []
            for v, val in zip(vs, tup):
                val = self.renameParams(val)
                eqs.append(mkEq(v, val))
            out.append(mkSet(eqs))
        return Seq(out)

    def renameParams(self, val):
        rep = {}
        for s in val.free_symbols:
            m = re.match(r'^t_(\d+)$', s.name)
            if m:
                rep[s] = sp.Symbol('_Z{}'.format(int(m.group(1)) + 1))
        return val.xreplace(rep)

    def doRsolve(self, a):
        needArgs(a, 1, 2)
        eqs, ineqs = self.splitEqs(a[0])
        init = {}
        rec = []
        fnc = a[1] if len(a) > 1 else None
        undef = sp.core.function.AppliedUndef
        for e in eqs:
            atoms = list(e.atoms(undef))
            if len(atoms) == 1 and all(x.is_number for x in atoms[0].args):
                vals = sp.solve(e, atoms[0])
                if len(vals) == 1:
                    init[atoms[0]] = vals[0]
                    continue
            rec.append(e)
        if not rec:
            raise MapleError('invalid recurrence')
        if fnc is None:
            fnc = list(rec[0].atoms(sp.core.function.AppliedUndef))[0]
        r = sp.rsolve(rec[0], fnc, init or None)
        if r is None:
            return self.unevaluated('rsolve', a)
        f0 = fnc.func
        rep = {}
        for s in r.free_symbols:
            m = re.match(r'^C(\d+)$', s.name)
            if m:
                rep[s] = f0(int(m.group(1)))
        return r.xreplace(rep)

    # ---- ODEs ----

    def isCondition(self, lhs):
        if isinstance(lhs, sp.Subs):
            return True
        if isinstance(lhs, sp.core.function.AppliedUndef) and all(x.is_number for x in lhs.args):
            return True
        return False

    def renameConsts(self, r, original):
        taken = set()
        for o in original:
            if isinstance(o, sp.Basic):
                taken |= {s.name for s in o.free_symbols}
        rep = {}

        def walk(x):
            for s in x.free_symbols:
                if isSym(s) and re.match(r'^C\d+$', s.name) and s.name not in taken:
                    rep[s] = sp.Symbol('_' + s.name)
        if isinstance(r, sp.Basic):
            walk(r)
            return r.xreplace(rep)
        return r

    def rebaseConditions(self, ics, funcs):
        """Rewrite D(y)(0) style keys in terms of the ODE's own independent variable."""
        out = {}
        for k, v in ics.items():
            if isinstance(k, sp.Subs) and isinstance(k.args[0], sp.Derivative):
                d = k.args[0]
                f = d.expr
                match = [fn for fn in funcs if isinstance(f, sp.core.function.AppliedUndef) and fn.func == f.func]
                if match and len(match[0].args) == 1 and isSym(match[0].args[0]):
                    x = match[0].args[0]
                    n = sum(int(c) for _, c in d.variable_count)
                    k = sp.Derivative(f.func(x), (x, n)).subs(x, k.args[2][0])
            out[k] = v
        return out

    def doDsolve(self, a):
        needArgs(a, 1)
        opts = [optName(o) for o in a[1:] if optName(o)]
        rest = [o for o in a[1:] if not optName(o) and not (isRel(o) and optName(relParts(o)[1]) in ('method', 'type', 'output', 'series', 'numeric'))]
        if 'numeric' in opts:
            raise MapleError('(in dsolve) numeric solutions are not supported yet')
        for o in a[1:]:
            if isRel(o) and optName(relParts(o)[1]) in ('type',) and optName(relParts(o)[2]) == 'numeric':
                raise MapleError('(in dsolve) numeric solutions are not supported yet')
        arg = a[0]
        items = list(arg) if isinstance(arg, (MSet, MList)) else [arg]
        odes, ics = [], {}
        for it in items:
            if isRel(it) and relParts(it)[0] == 'eq':
                op, l, r = relParts(it)
                if self.isCondition(l):
                    ics[l] = r
                    continue
                if self.isCondition(r):
                    ics[r] = l
                    continue
                odes.append(sp.Eq(l, r))
            elif isinstance(it, sp.Basic):
                odes.append(sp.Eq(it, 0))
            else:
                raise MapleError('invalid ODE')
        if not odes:
            raise MapleError('invalid ODE')
        funcs = None
        if rest:
            funcs = list(rest[0]) if isinstance(rest[0], (MSet, MList)) else [rest[0]]
        else:
            cand = []
            for o in odes:
                for f in o.atoms(sp.core.function.AppliedUndef):
                    if not any(f == c for c in cand):
                        cand.append(f)
            funcs = cand
        if not funcs:
            raise MapleError('unable to determine the dependent variable')
        ics = self.rebaseConditions(ics, funcs)
        try:
            if len(odes) == 1 and len(funcs) == 1:
                sol = sp.dsolve(odes[0], funcs[0], ics=ics or None)
            else:
                sol = sp.dsolve(odes, funcs, ics=ics or None)
        except NotImplementedError:
            return self.unevaluated('dsolve', a)
        except (ValueError, TypeError, AttributeError, NotImplementedError):
            return self.unevaluated('dsolve', a)
        if isinstance(sol, (list, tuple)):
            sol = [self.renameConsts(s, a) for s in sol]
            if len(odes) == 1 and len(funcs) == 1:
                return Seq(sol)
            return mkSet(sol)
        return self.renameConsts(sol, a)

    def doPdsolve(self, a):
        needArgs(a, 1, 2)
        pde = a[0]
        if isRel(pde):
            pde = sp.Eq(*relParts(pde)[1:])
        fn = a[1] if len(a) > 1 else list(pde.atoms(sp.core.function.AppliedUndef))[0]
        try:
            sol = sp.pdsolve(pde, fn)
        except NotImplementedError:
            return self.unevaluated('pdsolve', a)
        rep = {}
        for f in sol.atoms(sp.core.function.AppliedUndef):
            if f.func.__name__ == 'F':
                rep[f] = sp.Function('_F1')(*f.args)
        return sol.xreplace(rep)

    def doOdetest(self, a):
        needArgs(a, 2)
        sol, ode = a[0], a[1]
        sols = list(sol) if isinstance(sol, (MSet, MList, Seq)) else [sol]
        odes = list(ode) if isinstance(ode, (MSet, MList, Seq)) else [ode]
        funcs = {}
        for s in sols:
            if not isRel(s):
                raise MapleError('invalid solution')
            op, l, r = relParts(s)
            if not isinstance(l, sp.core.function.AppliedUndef):
                raise MapleError('invalid solution')
            funcs[l.func] = (l.args, r)

        def plug(e):
            args, val = funcs[e.func]
            return val.subs(dict(zip(args, e.args)), simultaneous=True)
        out = []
        for o in odes:
            expr = asExpr(o) if isRel(o) else o
            expr = expr.replace(lambda e: isinstance(e, sp.core.function.AppliedUndef) and e.func in funcs, plug)
            out.append(sp.simplify(expr.doit()))
        return out[0] if len(out) == 1 else Seq(out)


HANDLERS.update({
    'solve': 'doSolve', 'fsolve': 'doFsolve', 'isolve': 'doIsolve', 'rsolve': 'doRsolve',
    'dsolve': 'doDsolve', 'pdsolve': 'doPdsolve', 'odetest': 'doOdetest',
})


# ----------------------------------------------------------------------------
# Polynomials
# ----------------------------------------------------------------------------

def varList(e, spec):
    if spec is None:
        return sorted([s for s in e.free_symbols if isSym(s)], key=lambda s: s.name)
    if isinstance(spec, (MList, MSet)):
        return list(spec)
    return [spec]


class PolyMixin(object):
    def doCoeff(self, a):
        needArgs(a, 2, 3)
        p, x = a[0], a[1]
        n = a[2] if len(a) > 2 else sp.Integer(1)
        if len(a) == 2 and isinstance(x, sp.Pow) and x.exp.is_Integer:
            x, n = x.base, x.exp
        return sp.expand(p).coeff(x, n)

    def doCoeffs(self, a):
        needArgs(a, 1, 3)
        p = sp.expand(a[0])
        gens = varList(p, a[1] if len(a) > 1 and isinstance(a[1], (sp.Basic, MList, MSet)) else None)
        if not gens:
            return Seq([p])
        poly = sp.Poly(p, *gens)
        terms = sorted(poly.terms(), key=lambda t: t[0])
        return Seq([c for m, c in terms if c != 0])

    def doDegree(self, a):
        needArgs(a, 1, 2)
        p = a[0]
        if p == 0:
            return -sp.oo
        try:
            if len(a) == 2 and not isinstance(a[1], (MList, MSet)):
                return sp.Integer(sp.degree(p, a[1]))
            gens = varList(p, a[1] if len(a) > 1 else None)
            if not gens:
                return sp.Integer(0)
            return sp.Integer(sp.Poly(p, *gens).total_degree())
        except (sp.PolynomialError, sp.GeneratorsNeeded, ValueError, TypeError):
            return FAIL

    def doLdegree(self, a):
        needArgs(a, 1, 2)
        p = sp.expand(a[0])
        x = a[1] if len(a) > 1 else theVar(p)
        if p == 0:
            return sp.oo
        exps = [t.as_coeff_exponent(x)[1] for t in sp.Add.make_args(p)]
        return min(exps)

    def doLcoeff(self, a):
        needArgs(a, 1, 2)
        p = a[0]
        gens = varList(p, a[1] if len(a) > 1 else None)
        if not gens:
            return p
        return sp.Poly(p, *gens).LC()

    def doTcoeff(self, a):
        needArgs(a, 1, 2)
        p = sp.expand(a[0])
        x = a[1] if len(a) > 1 else theVar(p)
        ld = self.doLdegree([p, x])
        return p.coeff(x, ld)

    def doNumer(self, a):
        needArgs(a, 1)
        return mapS(lambda e: self.one(e, lambda x: sp.fraction(sp.together(x))[0]), a[0])

    def doDenom(self, a):
        needArgs(a, 1)
        return mapS(lambda e: self.one(e, lambda x: sp.fraction(sp.together(x))[1]), a[0])

    def doQuo(self, a):
        needArgs(a, 2, 4)
        x = a[2] if len(a) > 2 and isSym(a[2]) else theVar(a[0] * a[1])
        return sp.quo(a[0], a[1], x)

    def doRem(self, a):
        needArgs(a, 2, 4)
        x = a[2] if len(a) > 2 and isSym(a[2]) else theVar(a[0] * a[1])
        return sp.rem(a[0], a[1], x)

    def doGcd(self, a):
        needArgs(a, 2)
        r = a[0]
        for x in a[1:]:
            r = sp.gcd(r, x)
        return r

    def doLcm(self, a):
        needArgs(a, 2)
        r = a[0]
        for x in a[1:]:
            r = sp.lcm(r, x)
        return r

    def doResultant(self, a):
        needArgs(a, 3)
        return sp.resultant(a[0], a[1], a[2])

    def doDiscrim(self, a):
        needArgs(a, 2)
        return sp.discriminant(a[0], a[1])

    def contOf(self, p, x):
        poly = sp.Poly(p, x)
        return sp.gcd_list(poly.all_coeffs())

    def doContent(self, a):
        needArgs(a, 1, 2)
        p = sp.expand(a[0])
        x = a[1] if len(a) > 1 else theVar(p)
        return self.contOf(p, x)

    def doPrimpart(self, a):
        needArgs(a, 1, 2)
        p = sp.expand(a[0])
        x = a[1] if len(a) > 1 else theVar(p)
        c = self.contOf(p, x)
        return sp.expand(sp.cancel(p / c))

    def doSqrfree(self, a):
        needArgs(a, 1, 2)
        p = a[0]
        c, fl = sp.sqf_list(p)
        return MList([c, MList([MList([f, sp.Integer(m)]) for f, m in fl])])

    def doRoots(self, a):
        needArgs(a, 1, 3)
        p = a[0]
        var = None
        ext = False
        for o in a[1:]:
            if isSym(o):
                var = o
            else:
                ext = True
        var = theVar(p, var)
        poly = sp.Poly(p, var)
        rts = sp.roots(poly, filter=None if ext else 'Q')
        items = sorted(rts.items(), key=lambda kv: solKey(kv[0]))
        return MList([MList([r, sp.Integer(m)]) for r, m in items])

    def doRootOf(self, a):
        return rootOf(a)

    def doAllvalues(self, a):
        needArgs(a, 1)
        e = a[0]

        def vals(x):
            if isinstance(x, sp.CRootOf):
                z = sp.Dummy('z')
                poly = x.poly.as_expr().subs(x.poly.gen, z)
                rts = sp.roots(sp.Poly(poly, z))
                if sum(rts.values()) != x.poly.degree():
                    return Seq([x.evalf(self.digits + 5)])
                target = complex(x.evalf(30))
                for r in rts:
                    if abs(complex(r.evalf(30)) - target) < 1e-12:
                        return Seq([r])
                return Seq([x])
            if isinstance(x, sp.Function) and getattr(x.func, '__name__', '') == 'RootOf':
                poly = x.args[0]
                z = next((s for s in poly.free_symbols if s.name == '_Z'), theVar(poly))
                rts = sp.roots(sp.Poly(poly, z))
                if sum(rts.values()) != sp.Poly(poly, z).degree():
                    return Seq(list(sp.Poly(poly, z).nroots(n=self.digits + 5)))
                out = []
                for r, m in rts.items():
                    out.extend([r] * m)
                return Seq(orderSolutions(out))
            return Seq([x.replace(lambda q: isinstance(q, sp.CRootOf), lambda q: vals(q)[0])]) if isinstance(x, sp.Basic) else Seq([x])
        return vals(e)

    def doBasis(self, a):
        needArgs(a, 2)
        polys = list(a[0]) if isinstance(a[0], (MList, MSet)) else [a[0]]
        order = a[1]
        name = order.func.__name__ if isinstance(order, sp.core.function.AppliedUndef) else 'plex'
        gens = list(order.args) if isinstance(order, sp.core.function.AppliedUndef) else varList(sp.Add(*polys), None)
        omap = {'plex': 'lex', 'tdeg': 'grevlex', 'grlex': 'grlex', 'grevlex': 'grevlex', 'lex': 'lex'}
        G = sp.groebner([asExpr(p) for p in polys], *gens, order=omap.get(name, 'grevlex'))
        return MList(list(G.exprs))

    def doInterp(self, a):
        needArgs(a, 3)
        xs, ys, v = a
        pts = list(zip(list(xs), list(ys)))
        return sp.expand(sp.interpolate(pts, v))


HANDLERS.update({
    'coeff': 'doCoeff', 'coeffs': 'doCoeffs', 'degree': 'doDegree', 'ldegree': 'doLdegree',
    'lcoeff': 'doLcoeff', 'tcoeff': 'doTcoeff', 'numer': 'doNumer', 'denom': 'doDenom',
    'quo': 'doQuo', 'rem': 'doRem', 'gcd': 'doGcd', 'lcm': 'doLcm', 'resultant': 'doResultant',
    'discrim': 'doDiscrim', 'content': 'doContent', 'primpart': 'doPrimpart', 'sqrfree': 'doSqrfree',
    'roots': 'doRoots', 'RootOf': 'doRootOf', 'CRootOf': 'doRootOf', 'allvalues': 'doAllvalues', 'Basis': 'doBasis',
    'interp': 'doInterp',
})


# ----------------------------------------------------------------------------
# Number theory
# ----------------------------------------------------------------------------

def needInt(x):
    if isinstance(x, sp.Integer):
        return int(x)
    raise MapleError('wrong number (or type) of arguments')


class NumberMixin(object):
    def doIfactor(self, a):
        needArgs(a, 1)
        n = needInt(a[0])
        if n in (0, 1, -1):
            return a[0]
        fac = sp.factorint(abs(n))
        parts = []
        for p in sorted(fac):
            e = fac[p]
            if e == 1:
                parts.append({'int': str(p)})
            else:
                parts.append({'pow': [{'int': str(p)}, {'int': str(e)}]})
        if n < 0:
            parts.insert(0, {'int': '-1'})
        return Raw(parts[0] if len(parts) == 1 else {'mul': parts})

    def doIfactors(self, a):
        needArgs(a, 1)
        n = needInt(a[0])
        if n == 0:
            return MList([sp.Integer(0), MList([])])
        fac = sp.factorint(abs(n))
        return MList([sp.Integer(1 if n > 0 else -1),
                      MList([MList([sp.Integer(p), sp.Integer(fac[p])]) for p in sorted(fac)])])

    def doIsprime(self, a):
        needArgs(a, 1)
        return sp.true if sp.isprime(needInt(a[0])) else sp.false

    def doIthprime(self, a):
        needArgs(a, 1)
        n = needInt(a[0])
        if n < 1:
            raise MapleError('argument must be a positive integer')
        return sp.prime(n)

    def doNextprime(self, a):
        needArgs(a, 1)
        return sp.nextprime(needInt(a[0]))

    def doPrevprime(self, a):
        needArgs(a, 1)
        n = needInt(a[0])
        if n <= 2:
            raise MapleError('no previous prime')
        return sp.prevprime(n)

    def doIgcd(self, a):
        needArgs(a, 1)
        r = needInt(a[0])
        for x in a[1:]:
            r = math.gcd(r, needInt(x))
        return sp.Integer(abs(r))

    def doIlcm(self, a):
        needArgs(a, 1)
        r = needInt(a[0])
        for x in a[1:]:
            y = needInt(x)
            r = 0 if r == 0 or y == 0 else abs(r * y) // math.gcd(r, y)
        return sp.Integer(abs(r))

    def doIquo(self, a):
        needArgs(a, 2, 3)
        m, n = needInt(a[0]), needInt(a[1])
        if n == 0:
            raise MapleError('numeric exception: division by zero')
        q = abs(m) // abs(n)
        return sp.Integer(q if (m >= 0) == (n > 0) else -q)

    def doIrem(self, a):
        needArgs(a, 2, 3)
        m, n = needInt(a[0]), needInt(a[1])
        if n == 0:
            raise MapleError('numeric exception: division by zero')
        r = abs(m) % abs(n)
        return sp.Integer(r if m >= 0 else -r)

    def doIsqrt(self, a):
        needArgs(a, 1)
        n = needInt(a[0])
        if n < 0:
            raise MapleError('argument must be non-negative')
        return sp.Integer(math.isqrt(n))

    def doIroot(self, a):
        needArgs(a, 2)
        n, r = needInt(a[0]), needInt(a[1])
        return sp.Integer(int(sp.integer_nthroot(n, r)[0]))

    def residue(self, x, m, symmetric):
        if isinstance(x, sp.Integer):
            r = int(x) % m
            if symmetric and r > m // 2:
                r -= m
            return sp.Integer(r)
        if isinstance(x, sp.Rational):
            q = int(x.q)
            if math.gcd(q, m) != 1:
                raise MapleError('the modular inverse does not exist')
            r = (int(x.p) * pow(q, -1, m)) % m
            if symmetric and r > m // 2:
                r -= m
            return sp.Integer(r)
        if isinstance(x, sp.Basic) and x.free_symbols:
            gens = varList(x, None)
            return sp.Poly(x, *gens, modulus=m, symmetric=symmetric).as_expr()
        raise MapleError('wrong number (or type) of arguments')

    def modGeneric(self, a, symmetric):
        needArgs(a, 2)
        m = needInt(a[1])
        if m == 0:
            raise MapleError('numeric exception: division by zero')
        m = abs(m)
        return mapS(lambda x: self.residue(x, m, symmetric), a[0])

    def doMod(self, a):
        return self.modGeneric(a, False)

    def doMods(self, a):
        return self.modGeneric(a, True)

    def doPhi(self, a):
        needArgs(a, 1)
        return sp.totient(needInt(a[0]))

    def doSigma(self, a):
        needArgs(a, 1)
        return sp.divisor_sigma(needInt(a[0]))

    def doTau(self, a):
        needArgs(a, 1)
        return sp.divisor_count(needInt(a[0]))

    def doMobius(self, a):
        needArgs(a, 1)
        return sp.mobius(needInt(a[0]))

    def doDivisors(self, a):
        needArgs(a, 1)
        return mkSet([sp.Integer(d) for d in sp.divisors(needInt(a[0]))])


HANDLERS.update({
    'ifactor': 'doIfactor', 'ifactors': 'doIfactors', 'isprime': 'doIsprime', 'ithprime': 'doIthprime',
    'nextprime': 'doNextprime', 'prevprime': 'doPrevprime', 'igcd': 'doIgcd', 'ilcm': 'doIlcm',
    'iquo': 'doIquo', 'irem': 'doIrem', 'isqrt': 'doIsqrt', 'iroot': 'doIroot', 'mod': 'doMod',
    'modp': 'doMod', 'mods': 'doMods', 'phi': 'doPhi', 'sigma': 'doSigma', 'tau': 'doTau',
    'mobius': 'doMobius', 'divisors': 'doDivisors',
})


# ----------------------------------------------------------------------------
# Structure inspection: nops, op, type, indets ...
# ----------------------------------------------------------------------------

def operands(x):
    """Operands as Maple's op() sees them, in the same order the printer uses."""
    if isinstance(x, (MList, MSet, Seq)):
        return list(sortSet(x)) if isinstance(x, MSet) else list(x)
    if isinstance(x, Mat):
        return list(x.m)
    if isinstance(x, Rng):
        return [x.lo, x.hi]
    if isinstance(x, RelObj):
        return [x.lhs, x.rhs]
    if isinstance(x, sp.Integer) or isinstance(x, sp.Float):
        return [x]
    if isinstance(x, sp.Rational):
        return [sp.Integer(x.p), sp.Integer(x.q)]
    if isinstance(x, sp.Add):
        return orderedTerms(x)
    if isinstance(x, sp.Mul):
        return orderedFactors(x)
    if isinstance(x, sp.Pow):
        return [x.base, x.exp]
    if isinstance(x, sp.core.relational.Relational):
        op, l, r = relParts(x)
        return [l, r]
    if isinstance(x, sp.Derivative):
        out = [x.expr]
        for v, n in x.variable_count:
            out.extend([v] * int(n))
        return out
    if isinstance(x, sp.Piecewise):
        out = []
        for e, c in x.args:
            if c is sp.true and (e, c) == x.args[-1]:
                out.append(e)
            else:
                out.extend([c, e])
        return out
    if isinstance(x, sp.Function):
        return list(x.args)
    if isinstance(x, sp.Basic) and x.args and not x.is_Atom:
        return list(x.args)
    return [x]


def headOf(x):
    if isinstance(x, MList):
        return sp.Symbol('list')
    if isinstance(x, MSet):
        return sp.Symbol('set')
    if isinstance(x, Seq):
        return sp.Symbol('exprseq')
    if isinstance(x, Mat):
        return sp.Symbol('Matrix' if x.kind == 'matrix' else 'Vector')
    if isinstance(x, Rng):
        return sp.Symbol('..')
    if isinstance(x, Str):
        return sp.Symbol('string')
    if isinstance(x, RelObj):
        return sp.Symbol({'eq': '=', 'neq': '<>', 'lt': '<', 'le': '<='}[x.op])
    if isinstance(x, sp.Add):
        return sp.Symbol('+')
    if isinstance(x, sp.Mul):
        return sp.Symbol('*')
    if isinstance(x, sp.Pow):
        return sp.Symbol('^')
    if isinstance(x, sp.Integer):
        return sp.Symbol('integer')
    if isinstance(x, sp.Rational):
        return sp.Symbol('fraction')
    if isinstance(x, sp.Float):
        return sp.Symbol('float')
    if isinstance(x, sp.core.relational.Relational):
        return sp.Symbol({'==': '=', '!=': '<>', '<': '<', '<=': '<=', '>': '<', '>=': '<='}[x.rel_op])
    if isSym(x):
        return sp.Symbol('symbol')
    if isinstance(x, sp.Function):
        return sp.Symbol(REV.get(type(x), type(x).__name__))
    return sp.Symbol(type(x).__name__)


class MiscMixin(object):
    def doNops(self, a):
        needArgs(a, 1)
        x = a[0]
        if isinstance(x, sp.Basic) and (x.is_Atom or isinstance(x, (sp.Integer, sp.Float))):
            return sp.Integer(1)
        return sp.Integer(len(operands(x)))

    def doOp(self, a):
        needArgs(a, 1, 2)
        if len(a) == 1:
            return Seq(operands(a[0]))
        sel, x = a[0], a[1]
        ops = operands(x)
        if isinstance(sel, Rng):
            lo, hi = intOf(sel.lo), intOf(sel.hi)
            if lo < 1 or hi > len(ops):
                raise MapleError('invalid subscript selector')
            return Seq(ops[lo - 1:hi])
        i = intOf(sel)
        if i == 0:
            return headOf(x)
        if i < 0:
            i = len(ops) + i + 1
        if i < 1 or i > len(ops):
            raise MapleError('invalid subscript selector')
        return ops[i - 1]

    def doLhs(self, a):
        needArgs(a, 1)
        x = a[0]
        if isRel(x):
            return relParts(x)[1]
        if isinstance(x, Rng):
            return x.lo
        raise MapleError('wrong number (or type) of arguments')

    def doRhs(self, a):
        needArgs(a, 1)
        x = a[0]
        if isRel(x):
            return relParts(x)[2]
        if isinstance(x, Rng):
            return x.hi
        raise MapleError('wrong number (or type) of arguments')

    def collectIndets(self, x, out):
        if isinstance(x, (MList, MSet, Seq)):
            for i in x:
                self.collectIndets(i, out)
            return
        if isinstance(x, Mat):
            for i in x.m:
                self.collectIndets(i, out)
            return
        if isRel(x):
            op, l, r = relParts(x)
            self.collectIndets(l, out)
            self.collectIndets(r, out)
            return
        if isinstance(x, Rng):
            self.collectIndets(x.lo, out)
            self.collectIndets(x.hi, out)
            return
        if not isinstance(x, sp.Basic):
            return
        if isSym(x):
            out.append(x)
            return
        if x.is_Atom:
            return
        if isinstance(x, (sp.Add, sp.Mul, sp.Pow)) or isinstance(x, sp.core.relational.Relational):
            for i in x.args:
                self.collectIndets(i, out)
            return
        if isinstance(x, sp.Function):
            if x.free_symbols:
                out.append(x)
            for i in x.args:
                self.collectIndets(i, out)
            return
        for i in x.args:
            self.collectIndets(i, out)

    def doIndets(self, a):
        needArgs(a, 1, 2)
        found = []
        self.collectIndets(a[0], found)
        if len(a) == 2:
            t = a[1]
            found = [f for f in found if self.typeCheck(f, t) is True]
        return mkSet(found)

    def doHas(self, a):
        needArgs(a, 2)
        e, pat = a
        pats = list(pat) if isinstance(pat, (MList, MSet)) else [pat]

        def has1(x, p):
            if isinstance(x, (MList, MSet, Seq)):
                return any(has1(i, p) for i in x)
            if isinstance(x, Mat):
                return any(has1(i, p) for i in x.m)
            if isRel(x):
                op, l, r = relParts(x)
                return has1(l, p) or has1(r, p)
            if isinstance(x, sp.Basic) and isinstance(p, sp.Basic):
                return x.has(p)
            return sameObj(x, p)
        return sp.true if any(has1(e, p) for p in pats) else sp.false

    def doType(self, a):
        needArgs(a, 2)
        r = self.typeCheck(a[0], a[1])
        return sp.true if r else sp.false

    def typeCheck(self, e, t):
        if isinstance(t, (MSet, MList)):
            return any(self.typeCheck(e, x) for x in t)
        name = optName(t)
        if name is None and isinstance(t, sp.Function):
            name = t.func.__name__
        if name is None:
            return False
        sub = t.args if isinstance(t, sp.Function) else ()
        isB = isinstance(e, sp.Basic)
        if name in ('anything',):
            return True
        if name == 'integer':
            return isinstance(e, sp.Integer)
        if name == 'posint':
            return isinstance(e, sp.Integer) and e > 0
        if name == 'negint':
            return isinstance(e, sp.Integer) and e < 0
        if name == 'nonnegint':
            return isinstance(e, sp.Integer) and e >= 0
        if name == 'nonposint':
            return isinstance(e, sp.Integer) and e <= 0
        if name == 'rational':
            return isinstance(e, sp.Rational)
        if name == 'fraction':
            return isinstance(e, sp.Rational) and not isinstance(e, sp.Integer)
        if name == 'float':
            return isinstance(e, sp.Float)
        if name == 'numeric':
            return isinstance(e, (sp.Rational, sp.Float))
        if name == 'realcons':
            return isB and e.is_number and e.is_real is True
        if name in ('complex', 'complexcons'):
            return isB and e.is_number and e.is_finite is not False and not e.has(sp.oo, sp.zoo, sp.nan)
        if name == 'constant':
            return isB and e.is_number
        if name in ('symbol', 'name'):
            return isSym(e)
        if name == 'algebraic':
            return isB and not isRel(e)
        if name in ('list',):
            return isinstance(e, MList)
        if name == 'set':
            return isinstance(e, MSet)
        if name == 'exprseq':
            return isinstance(e, Seq)
        if name in ('equation', 'eq'):
            return isRel(e) and relParts(e)[0] == 'eq'
        if name in ('relation',):
            return isRel(e)
        if name in ('less',):
            return isRel(e) and relParts(e)[0] == 'lt'
        if name == 'positive':
            return isB and e.is_positive is True
        if name == 'negative':
            return isB and e.is_negative is True
        if name in ('nonneg', 'nonnegative'):
            return isB and e.is_nonnegative is True
        if name == 'even':
            return isB and e.is_even is True
        if name == 'odd':
            return isB and e.is_odd is True
        if name == 'prime':
            return isinstance(e, sp.Integer) and sp.isprime(int(e))
        if name == 'Matrix':
            return isinstance(e, Mat) and e.kind == 'matrix'
        if name == 'Vector':
            return isinstance(e, Mat) and e.kind in ('col', 'row')
        if name == 'string':
            return isinstance(e, Str)
        if name == 'boolean':
            return e is sp.true or e is sp.false
        if name == 'range':
            return isinstance(e, Rng)
        if name == 'function':
            return isinstance(e, sp.Function)
        if name == 'procedure':
            return isinstance(e, sp.Lambda)
        if name == '+':
            return isinstance(e, sp.Add)
        if name == '*':
            return isinstance(e, sp.Mul)
        if name == '^':
            return isinstance(e, sp.Pow)
        if name == 'polynom':
            if not isB:
                return False
            if sub:
                vs = [s for s in sub if isinstance(s, sp.Basic) and not (optName(s) in TYPE_PROPS or optName(s) == 'anything')]
                vs = [s for s in vs if isSym(s) or isinstance(s, sp.core.function.AppliedUndef)]
                if vs:
                    return e.is_polynomial(*vs)
            fs = [s for s in e.free_symbols if isSym(s)]
            return e.is_polynomial(*fs) if fs else (e.is_number and not e.has(sp.zoo, sp.nan))
        if name == 'specfunc':
            if not isinstance(e, sp.Function):
                return False
            fname = REV.get(type(e), type(e).__name__)
            wanted = [optName(s) for s in sub[1:]] if len(sub) > 1 else [optName(sub[0])] if sub else []
            return not wanted or fname in wanted
        if name == 'radnum':
            return isB and e.is_number and e.is_real is not False
        if name == 'trig':
            return isinstance(e, (sp.sin, sp.cos, sp.tan, sp.cot, sp.sec, sp.csc))
        return False

    def doWhattype(self, a):
        needArgs(a, 1)
        x = a[0]
        if isinstance(x, sp.Basic) and x.is_number and not isinstance(x, (sp.Rational, sp.Float)) \
                and x.is_Add and x.has(sp.I):
            return sp.Symbol('complex')
        return headOf(x)

    def doUnapply(self, a):
        needArgs(a, 2)
        vs = list(a[1]) if isinstance(a[1], (MList, MSet)) else list(a[1:])
        return sp.Lambda(tuple(vs), a[0])

    def doIsolate(self, a):
        needArgs(a, 2)
        eq, x = a
        e = asExpr(eq)
        sols = sp.solve(e, x, dict=True)
        if not sols:
            return self.unevaluated('isolate', a)
        return mkEq(x, sols[0][x])


HANDLERS.update({
    'nops': 'doNops', 'op': 'doOp', 'lhs': 'doLhs', 'rhs': 'doRhs', 'indets': 'doIndets',
    'has': 'doHas', 'type': 'doType', 'whattype': 'doWhattype', 'unapply': 'doUnapply',
    'isolate': 'doIsolate',
})


# ----------------------------------------------------------------------------
# Linear algebra
# ----------------------------------------------------------------------------

def asM(x):
    """Matrix-like (Mat, list of lists, list) -> sympy Matrix."""
    if isinstance(x, Mat):
        return x.m
    if isinstance(x, (MList, Seq)):
        items = list(x)
        if items and all(isinstance(r, (MList, list)) for r in items):
            return sp.Matrix([list(r) for r in items])
        return sp.Matrix(items)
    raise MapleError('wrong number (or type) of arguments')


def colVec(items):
    return Mat(sp.Matrix(list(items)) if len(list(items)) else sp.zeros(0, 1), 'col')


def eigKey(v):
    try:
        c = complex(v.evalf(25))
        return (round(c.real, 12), round(c.imag, 12))
    except (TypeError, ValueError, AttributeError):
        return (0.0, 0.0)


class LinAlgMixin(object):
    def fillValue(self, a):
        for o in a:
            if isRel(o):
                op, l, r = relParts(o)
                if optName(l) == 'fill':
                    return r
        return sp.Integer(0)

    def plainArgs(self, a):
        return [o for o in a if not (isRel(o) and optName(relParts(o)[1]) in ('fill', 'shape', 'datatype', 'storage', 'order', 'readonly', 'attributes', 'orientation', 'scan', 'symbol', 'outputoptions'))]

    def doMatrix(self, a):
        fill = self.fillValue(a)
        args = self.plainArgs(a)
        if not args:
            return Mat(sp.zeros(0, 0))
        first = args[0]
        if isinstance(first, (MList, Seq)) and not isinstance(first, (Mat,)):
            if first and all(isinstance(r, (MList, list)) for r in first):
                return Mat(sp.Matrix([list(r) for r in first]))
            return Mat(sp.Matrix(list(first)))
        if isinstance(first, Mat):
            return Mat(first.m)
        r = intOf(first)
        c = intOf(args[1]) if len(args) > 1 and isinstance(args[1], sp.Integer) else r
        data = args[2] if len(args) > 2 else (args[1] if len(args) > 1 and not isinstance(args[1], sp.Integer) else None)
        if r < 0 or c < 0:
            raise MapleError('invalid dimensions')
        m = sp.zeros(r, c)
        if fill != 0:
            m = sp.Matrix(r, c, lambda i, j: fill)
        if data is None:
            return Mat(m)
        if isinstance(data, sp.Lambda):
            return Mat(sp.Matrix(r, c, lambda i, j: data(sp.Integer(i + 1), sp.Integer(j + 1))))
        if isinstance(data, (MList, Seq)):
            items = list(data)
            if items and all(isinstance(x, (MList, list)) for x in items):
                for i, row in enumerate(items):
                    for j, v in enumerate(row):
                        if i >= r or j >= c:
                            raise MapleError('initializer defines too many entries')
                        m[i, j] = v
            else:
                if len(items) > r * c:
                    raise MapleError('initializer defines too many entries')
                for k, v in enumerate(items):
                    m[k // c, k % c] = v
            return Mat(m)
        if isinstance(data, sp.Basic):
            return Mat(sp.Matrix(r, c, lambda i, j: data))
        raise MapleError('wrong number (or type) of arguments')

    def orient(self):
        for i in self.callIdx:
            if optName(i) == 'row':
                return 'row'
        return 'col'

    def doVector(self, a):
        fill = self.fillValue(a)
        args = self.plainArgs(a)
        kind = self.orient()
        for o in a:
            if isSym(o) and o.name in ('row', 'column'):
                kind = 'row' if o.name == 'row' else 'col'
                args = [x for x in args if x is not o]
        if not args:
            items = []
        elif isinstance(args[0], (MList, Seq)):
            items = list(args[0])
        elif isinstance(args[0], Mat):
            items = list(args[0].m)
        else:
            n = intOf(args[0])
            items = [fill] * n
            if len(args) > 1:
                if isinstance(args[1], sp.Lambda):
                    items = [args[1](sp.Integer(i + 1)) for i in range(n)]
                elif isinstance(args[1], (MList, Seq)):
                    items = list(args[1]) + [fill] * (n - len(args[1]))
                elif isinstance(args[1], sp.Basic):
                    items = [args[1]] * n
        if kind == 'row':
            return Mat(sp.Matrix([items]) if items else sp.zeros(1, 0), 'row')
        return Mat(sp.Matrix(items) if items else sp.zeros(0, 1), 'col')

    def doRowVector(self, a):
        self.callIdx = [sp.Symbol('row')]
        return self.doVector(a)

    def doIdentityMatrix(self, a):
        needArgs(a, 1, 2)
        n = intOf(a[0])
        m = intOf(a[1]) if len(a) > 1 else n
        return Mat(sp.eye(n, m))

    def doZeroMatrix(self, a):
        needArgs(a, 1, 2)
        return Mat(sp.zeros(intOf(a[0]), intOf(a[1]) if len(a) > 1 else intOf(a[0])))

    def doDiagonalMatrix(self, a):
        needArgs(a, 1, 3)
        items = list(a[0]) if isinstance(a[0], (MList, Seq)) else list(asM(a[0]))
        n = len(items)
        r = intOf(a[1]) if len(a) > 1 else n
        c = intOf(a[2]) if len(a) > 2 else r
        m = sp.zeros(r, c)
        for i, v in enumerate(items):
            if i < r and i < c:
                m[i, i] = v
        return Mat(m)

    def sq(self, A, name):
        m = asM(A)
        if m.rows != m.cols:
            raise MapleError('(in LinearAlgebra:-{}) expects a square Matrix'.format(name))
        return m

    def doDeterminant(self, a):
        needArgs(a, 1)
        return self.sq(a[0], 'Determinant').det()

    def doMatrixInverse(self, a):
        needArgs(a, 1)
        m = self.sq(a[0], 'MatrixInverse')
        if m.det() == 0:
            raise MapleError('(in LinearAlgebra:-MatrixInverse) singular matrix')
        return Mat(m.inv(), 'matrix')

    def doTranspose(self, a):
        needArgs(a, 1)
        x = a[0]
        m = asM(x)
        kind = {'col': 'row', 'row': 'col'}.get(x.kind if isinstance(x, Mat) else 'matrix', 'matrix')
        return Mat(m.T, kind)

    def doHermitian(self, a):
        needArgs(a, 1)
        x = a[0]
        m = asM(x)
        kind = {'col': 'row', 'row': 'col'}.get(x.kind if isinstance(x, Mat) else 'matrix', 'matrix')
        return Mat(m.H, kind)

    def doTrace(self, a):
        needArgs(a, 1)
        return self.sq(a[0], 'Trace').trace()

    def doRank(self, a):
        needArgs(a, 1)
        return sp.Integer(asM(a[0]).rank())

    def eigPairs(self, m):
        try:
            vals = m.eigenvals()
        except (NotImplementedError, sp.matrices.exceptions.MatrixError, ValueError):
            raise MapleError('unable to compute eigenvalues')
        realMatrix = all(isinstance(x, sp.Rational) for x in m)
        if realMatrix and any(v.has(sp.I) for v in vals) and all(eigKey(v)[1] == 0 for v in vals):
            vals = self.rootOfEigenvalues(m)
        return sorted(vals.items(), key=lambda kv: eigKey(kv[0]))

    def rootOfEigenvalues(self, m):
        """Real eigenvalues of a rational matrix as RootOf objects (radicals would need complex numbers)."""
        lam = sp.Dummy('lam')
        out = {}
        c, factors = sp.factor_list(m.charpoly(lam).as_expr(lam), lam)
        for f, mult in factors:
            poly = sp.Poly(f, lam)
            if poly.degree() <= 2:
                for r, k in sp.roots(poly).items():
                    out[r] = out.get(r, 0) + k * mult
            else:
                for i in range(poly.degree()):
                    r = sp.CRootOf(poly, i)
                    out[r] = out.get(r, 0) + mult
        return out

    def floatEig(self, m, vectors=False):
        """Eigen-decomposition of a numeric Matrix with floats (Maple uses LAPACK here)."""
        if not m.has(sp.Float) or m.free_symbols:
            return None
        D = max(self.digits, 15)
        self.cv.printDigits = max(self.cv.printDigits, D)
        with mpmath.workdps(D + 10):
            A = mpmath.matrix([[mpmath.mpmathify(complex(x)) if not x.is_real else mpmath.mpf(str(sp.Float(x, D + 10)))
                                for x in m.row(i)] for i in range(m.rows)])
            if all(x.is_real for x in m) and m == m.T:
                E, Q = mpmath.eigsy(A)
                vals = [sp.Float(str(E[i]), D) for i in range(m.rows)]
            else:
                E, Q = mpmath.eig(A)
                order = sorted(range(m.rows), key=lambda i: (mpmath.re(E[i]), mpmath.im(E[i])))
                E = [E[i] for i in order]
                Q = mpmath.matrix([[Q[r, c] for c in order] for r in range(m.rows)])
                vals = []
                for i in range(m.rows):
                    re_, im_ = sp.Float(str(mpmath.re(E[i])), D), sp.Float(str(mpmath.im(E[i])), D)
                    vals.append(re_ + im_ * sp.I if im_ != 0 else re_)
            if not vectors:
                return colVec(vals)
            cols = [[sp.Float(str(mpmath.re(Q[r, c])), D) + sp.Float(str(mpmath.im(Q[r, c])), D) * sp.I
                     if mpmath.im(Q[r, c]) != 0 else sp.Float(str(mpmath.re(Q[r, c])), D)
                     for r in range(m.rows)] for c in range(m.cols)]
            return Seq([colVec(vals), Mat(sp.Matrix(cols).T)])

    def doEigenvalues(self, a):
        needArgs(a, 1)
        m = self.sq(a[0], 'Eigenvalues')
        num = self.floatEig(m)
        if num is not None:
            return num
        out = []
        for v, mult in self.eigPairs(m):
            out.extend([v] * int(mult))
        return colVec(out)

    def doEigenvectors(self, a):
        needArgs(a, 1)
        m = self.sq(a[0], 'Eigenvectors')
        num = self.floatEig(m, vectors=True)
        if num is not None:
            return num
        pairs = self.eigPairs(m)
        vals, cols = [], []
        for v, mult in pairs:
            ns = (m - v * sp.eye(m.rows)).nullspace()
            ns = [self.tidy(vec) for vec in ns]
            for k in range(int(mult)):
                vals.append(v)
                cols.append(ns[k] if k < len(ns) else sp.zeros(m.rows, 1))
        E = sp.Matrix.hstack(*cols) if cols else sp.zeros(m.rows, 0)
        return Seq([colVec(vals), Mat(E)])

    def tidy(self, vec):
        ok, r = timeLimited(5, lambda: vec.applyfunc(sp.simplify))
        return r if ok and r is not None else vec

    def doNullSpace(self, a):
        needArgs(a, 1)
        return mkSet([Mat(v, 'col') for v in asM(a[0]).nullspace()])

    def doColumnSpace(self, a):
        needArgs(a, 1)
        return MList([Mat(v, 'col') for v in asM(a[0]).columnspace()])

    def doRowSpace(self, a):
        needArgs(a, 1)
        return MList([Mat(v, 'row') for v in asM(a[0]).rowspace()])

    def doLinearSolve(self, a):
        needArgs(a, 2)
        A, b = asM(a[0]), asM(a[1])
        try:
            sol, params = A.gauss_jordan_solve(b)
        except ValueError:
            raise MapleError('(in LinearAlgebra:-LinearSolve) inconsistent system')
        rep = {p: sp.Symbol('_t{}'.format(i + 1)) for i, p in enumerate(params)}
        sol = sol.xreplace(rep)
        kind = a[1].kind if isinstance(a[1], Mat) and a[1].kind == 'col' else 'matrix'
        return Mat(sol, kind)

    def doRref(self, a):
        needArgs(a, 1)
        return Mat(asM(a[0]).rref()[0])

    def doGaussian(self, a):
        needArgs(a, 1)
        return Mat(asM(a[0]).echelon_form())

    def doLU(self, a):
        needArgs(a, 1)
        m = self.sq(a[0], 'LUDecomposition')
        L, U, perm = m.LUdecomposition()
        Pm = sp.eye(m.rows)
        for i, j in perm:
            Pm.row_swap(i, j)
        return Seq([Mat(Pm.T), Mat(L), Mat(U)])

    def doQR(self, a):
        needArgs(a, 1)
        Q, R = asM(a[0]).QRdecomposition()
        return Seq([Mat(Q), Mat(R)])

    def doCharPoly(self, a):
        needArgs(a, 2)
        return self.sq(a[0], 'CharacteristicPolynomial').charpoly(a[1]).as_expr()

    def doMinPoly(self, a):
        needArgs(a, 2)
        m = self.sq(a[0], 'MinimalPolynomial')
        x = a[1]
        n = m.rows
        powers = [sp.eye(n)]
        for k in range(1, n + 1):
            powers.append(powers[-1] * m)
            M = sp.Matrix.hstack(*[p.reshape(n * n, 1) for p in powers])
            ns = M.nullspace()
            if ns:
                v = ns[0]
                v = v / v[len(v) - 1] if v[len(v) - 1] != 0 else v
                return sp.expand(sum(v[i] * x ** i for i in range(len(v))))
        raise MapleError('unable to compute the minimal polynomial')

    def doJordan(self, a):
        needArgs(a, 1)
        P, J = self.sq(a[0], 'JordanForm').jordan_form()
        return Mat(J)

    def doDot(self, a):
        needArgs(a, 2)
        u, v = asM(a[0]), asM(a[1])
        if len(u) != len(v):
            raise MapleError('(in LinearAlgebra:-DotProduct) vector dimensions must be the same')
        return sum(sp.conjugate(x) * y for x, y in zip(list(u), list(v)))

    def doCross(self, a):
        needArgs(a, 2)
        u, v = asM(a[0]), asM(a[1])
        if len(u) != 3 or len(v) != 3:
            raise MapleError('(in LinearAlgebra:-CrossProduct) vectors must have dimension 3')
        kind = a[0].kind if isinstance(a[0], Mat) and a[0].kind in ('col', 'row') else 'col'
        w = sp.Matrix(u).cross(sp.Matrix(v))
        return Mat(w if kind == 'col' else w.T, kind)

    def normOf(self, x, p):
        m = asM(x)
        isVec = isinstance(x, Mat) and x.kind in ('col', 'row') or not isinstance(x, Mat)
        pn = optName(p) if p is not None else None
        if isVec:
            items = list(m)
            if p is None or p == sp.oo or pn == 'infinity':
                return sp.Max(*[sp.Abs(i) for i in items])
            if pn == 'Frobenius' or p == 2:
                return sp.sqrt(sum(sp.Abs(i) ** 2 for i in items))
            if p == 1:
                return sum(sp.Abs(i) for i in items)
            return sum(sp.Abs(i) ** p for i in items) ** (1 / p)
        if p is None or p == sp.oo:
            return sp.Max(*[sum(sp.Abs(m[i, j]) for j in range(m.cols)) for i in range(m.rows)])
        if p == 1:
            return sp.Max(*[sum(sp.Abs(m[i, j]) for i in range(m.rows)) for j in range(m.cols)])
        if pn == 'Frobenius':
            return sp.sqrt(sum(sp.Abs(i) ** 2 for i in list(m)))
        if p == 2:
            return sp.Max(*m.singular_values())
        raise MapleError('invalid norm')

    def doNorm(self, a):
        needArgs(a, 1, 2)
        return self.normOf(a[0], a[1] if len(a) > 1 else None)

    def doNormalize(self, a):
        needArgs(a, 1, 2)
        x = a[0]
        p = a[1] if len(a) > 1 else sp.Integer(2)
        n = self.normOf(x, p)
        return Mat(asM(x) / n, x.kind if isinstance(x, Mat) else 'col')

    def doMatExp(self, a):
        needArgs(a, 1, 2)
        m = self.sq(a[0], 'MatrixExponential')
        t = a[1] if len(a) > 1 else 1
        return Mat((m * t).exp())

    def doMatPow(self, a):
        needArgs(a, 2)
        m = self.sq(a[0], 'MatrixPower')
        return Mat(m ** intOf(a[1]))

    def doIsDefinite(self, a):
        needArgs(a, 1, 2)
        m = self.sq(a[0], 'IsDefinite')
        q = 'positive_definite'
        for o in a[1:]:
            if isRel(o):
                q = optName(relParts(o)[2]) or q
        if q == 'positive_definite':
            return self.triBool(m.is_positive_definite)
        if q == 'positive_semidefinite':
            return self.triBool(m.is_positive_semidefinite)
        if q == 'negative_definite':
            return self.triBool(m.is_negative_definite)
        if q == 'negative_semidefinite':
            return self.triBool(m.is_negative_semidefinite)
        return FAIL

    def doGramSchmidt(self, a):
        needArgs(a, 1, 2)
        vecs = [asM(v) for v in a[0]]
        normalized = any(optName(o) == 'normalized' for o in a[1:])
        out = sp.GramSchmidt(vecs, orthonormal=normalized)
        return MList([Mat(v, 'col') for v in out])

    def doSingularValues(self, a):
        needArgs(a, 1)
        sv = asM(a[0]).singular_values()
        return colVec(sorted(sv, key=lambda v: -eigKey(v)[0]))

    def doDimension(self, a):
        needArgs(a, 1)
        m = asM(a[0])
        if isinstance(a[0], Mat) and a[0].kind != 'matrix':
            return sp.Integer(len(m))
        return Seq([sp.Integer(m.rows), sp.Integer(m.cols)])

    def doRowDim(self, a):
        needArgs(a, 1)
        return sp.Integer(asM(a[0]).rows)

    def doColDim(self, a):
        needArgs(a, 1)
        return sp.Integer(asM(a[0]).cols)

    def doMultiply(self, a):
        needArgs(a, 2)
        acc = a[0]
        for x in a[1:]:
            acc = opMul([acc, x]) if not (isinstance(acc, Mat) and isinstance(x, Mat)) else matMul(acc, x)
        return acc

    def nested(self, name, a, idx=None):
        saved = self.callIdx
        self.callIdx = idx or []
        try:
            if name not in HANDLERS:
                raise MapleError('unsupported nested call {}'.format(name))
            return getattr(self, HANDLERS[name])(a)
        finally:
            self.callIdx = saved


HANDLERS.update({
    'Matrix': 'doMatrix', 'Vector': 'doVector', 'IdentityMatrix': 'doIdentityMatrix',
    'ZeroMatrix': 'doZeroMatrix', 'DiagonalMatrix': 'doDiagonalMatrix', 'Determinant': 'doDeterminant',
    'MatrixInverse': 'doMatrixInverse', 'Transpose': 'doTranspose', 'HermitianTranspose': 'doHermitian',
    'Trace': 'doTrace', 'Rank': 'doRank', 'Eigenvalues': 'doEigenvalues', 'Eigenvectors': 'doEigenvectors',
    'NullSpace': 'doNullSpace', 'ColumnSpace': 'doColumnSpace', 'RowSpace': 'doRowSpace',
    'LinearSolve': 'doLinearSolve', 'ReducedRowEchelonForm': 'doRref',
    'GaussianElimination': 'doGaussian', 'LUDecomposition': 'doLU', 'QRDecomposition': 'doQR',
    'CharacteristicPolynomial': 'doCharPoly', 'MinimalPolynomial': 'doMinPoly', 'JordanForm': 'doJordan',
    'DotProduct': 'doDot', 'CrossProduct': 'doCross', 'Norm': 'doNorm', 'Normalize': 'doNormalize',
    'MatrixExponential': 'doMatExp', 'MatrixPower': 'doMatPow', 'IsDefinite': 'doIsDefinite',
    'GramSchmidt': 'doGramSchmidt', 'SingularValues': 'doSingularValues', 'Dimension': 'doDimension',
    'RowDimension': 'doRowDim', 'ColumnDimension': 'doColDim', 'Multiply': 'doMultiply',
    'MatrixMatrixMultiply': 'doMultiply', 'MatrixVectorMultiply': 'doMultiply', '.': 'doMultiply',
})


# ----------------------------------------------------------------------------
# Transforms and vector calculus
# ----------------------------------------------------------------------------

class TransformMixin(object):
    def dropHeaviside(self, r, t):
        return r.replace(lambda x: isinstance(x, sp.Heaviside) and x.args[0] == t, lambda x: sp.Integer(1))

    def doLaplace(self, a):
        needArgs(a, 3)
        f, t, s = a

        def go(x):
            if not isinstance(x, sp.Basic):
                return x
            r = sp.laplace_transform(x, t, s, noconds=True)
            if isinstance(r, tuple):
                r = r[0]
            if isinstance(r, sp.LaplaceTransform):
                return None
            return r
        res = mapS(lambda x: go(x) if go(x) is not None else x, f) if isinstance(f, (MList, MSet)) else go(f)
        if res is None:
            return self.unevaluated('laplace', a)
        return res

    def doInvlaplace(self, a):
        needArgs(a, 3)
        F, s, t = a
        r = sp.inverse_laplace_transform(F, s, t)
        if isinstance(r, sp.InverseLaplaceTransform):
            return self.unevaluated('invlaplace', a)
        return self.dropHeaviside(r, t)

    def doFourier(self, a):
        needArgs(a, 3)
        f, t, w = a
        k = sp.Dummy('k')
        r = sp.fourier_transform(f, t, k)
        if isinstance(r, sp.FourierTransform) or r.has(sp.FourierTransform):
            return self.unevaluated('fourier', a)
        return r.subs(k, w / (2 * sp.pi))

    def doInvfourier(self, a):
        needArgs(a, 3)
        F, w, t = a
        k = sp.Dummy('k')
        r = sp.inverse_fourier_transform(F.subs(w, 2 * sp.pi * k), k, t)
        if isinstance(r, sp.InverseFourierTransform) or r.has(sp.InverseFourierTransform):
            return self.unevaluated('invfourier', a)
        return r

    def doMellin(self, a):
        needArgs(a, 3)
        f, x, s = a
        r = sp.mellin_transform(f, x, s)
        if isinstance(r, sp.MellinTransform):
            return self.unevaluated('mellin', a)
        return r[0] if isinstance(r, tuple) else r


class VecCalcMixin(object):
    def vecVars(self, v):
        if isinstance(v, (MList, MSet, Seq)):
            return list(v)
        if isinstance(v, Mat):
            return list(v.m)
        return [v]

    def doGradient(self, a):
        needArgs(a, 2)
        vs = self.vecVars(a[1])
        return colVec([sp.diff(a[0], v) for v in vs])

    def doDivergence(self, a):
        needArgs(a, 2)
        F = list(asM(a[0]))
        vs = self.vecVars(a[1])
        if len(F) != len(vs):
            raise MapleError('dimension mismatch')
        return sum(sp.diff(f, v) for f, v in zip(F, vs))

    def doCurl(self, a):
        needArgs(a, 2)
        F = list(asM(a[0]))
        vs = self.vecVars(a[1])
        if len(F) != 3 or len(vs) != 3:
            raise MapleError('curl requires a three-dimensional field')
        x, y, z = vs
        return colVec([sp.diff(F[2], y) - sp.diff(F[1], z), sp.diff(F[0], z) - sp.diff(F[2], x),
                       sp.diff(F[1], x) - sp.diff(F[0], y)])

    def doLaplacian(self, a):
        needArgs(a, 2)
        vs = self.vecVars(a[1])
        f = a[0]
        if isinstance(f, Mat):
            return colVec([sum(sp.diff(c, v, 2) for v in vs) for c in list(f.m)])
        return sum(sp.diff(f, v, 2) for v in vs)

    def doJacobian(self, a):
        needArgs(a, 2)
        F = list(asM(a[0]))
        vs = self.vecVars(a[1])
        return Mat(sp.Matrix(F).jacobian(vs))

    def doHessian(self, a):
        needArgs(a, 2)
        vs = self.vecVars(a[1])
        return Mat(sp.hessian(a[0], vs))

    def doWronskian(self, a):
        needArgs(a, 2)
        fs = list(asM(a[0]))
        x = a[1]
        n = len(fs)
        return Mat(sp.Matrix(n, n, lambda i, j: sp.diff(fs[j], x, i) if i > 0 else fs[j]))


HANDLERS.update({
    'laplace': 'doLaplace', 'invlaplace': 'doInvlaplace', 'fourier': 'doFourier',
    'invfourier': 'doInvfourier', 'mellin': 'doMellin',
    'Gradient': 'doGradient', 'Divergence': 'doDivergence', 'Curl': 'doCurl', 'Laplacian': 'doLaplacian',
    'Jacobian': 'doJacobian', 'Hessian': 'doHessian', 'Wronskian': 'doWronskian',
})


# ==== HANDLER MIXINS GO ABOVE THIS LINE ====

class Engine(ArithMixin, CalculusMixin, SolveMixin, PolyMixin, NumberMixin, MiscMixin, LinAlgMixin,
             TransformMixin, VecCalcMixin, VerifyMixin, DomainMixin, RigorMixin, StepMixin, PrintMixin):
    """Request dispatcher: protocol, conversion context, registry, verify, printing."""

    def __init__(self):
        self.digits = 10
        self.served = 0
        self.callOrder = 1
        self.callIdx = []
        self.assume = Assumptions([])
        self.cv = Convert(None, 10)
        self.cv.engine = self

    def setup(self, opts):
        opts = opts or {}
        d = opts.get('digits', 10)
        try:
            d = max(1, min(int(d), 100000))
        except (TypeError, ValueError):
            d = 10
        self.digits = d
        self.assume = Assumptions(opts.get('assume') or [])
        self.cv = Convert(self.assume, d)
        self.cv.engine = self

    def handle(self, req):
        self.served += 1
        if self.served % 200 == 0:
            sp.core.cache.clear_cache()
        op = req.get('op')
        if op == 'ping':
            return {'ok': True, 'result': {'str': 'pong'}}
        if op == 'call':
            return self.opCall(req)
        if op == 'verify':
            return self.opVerify(req)
        if op == 'check_step':
            return self.opCheckStep(req)
        if op == 'check_solution':
            return self.opCheckSolution(req)
        if op == 'latex':
            return self.opLatex(req)
        if op == 'pretty':
            return self.opPretty(req)
        return {'ok': False, 'error': 'unknown operation {}'.format(op)}

    def opCall(self, req):
        name = req.get('name')
        args = req.get('args', [])
        self.setup(req.get('opts'))
        short = shortName(name)
        self.callOrder = 1
        if isinstance(name, str):
            m = re.match(r'^\(?D@@(\d+)\)?$', name)
            if m:
                short, self.callOrder = 'D', int(m.group(1))
        try:
            self.curName = short
            if short not in HANDLERS and short not in FUNCS:
                return {'ok': True, 'result': {'fn': name, 'args': args}}
            self.callIdx = [self.cv.toSympy(i) for i in (req.get('idx') or [])]
            a = self.cv.flat([self.cv.toSympy(x) for x in args])
            if short in FUNCS and short not in HANDLERS:
                res = floatContagion(FUNCS[short](*a))
            else:
                res = getattr(self, HANDLERS[short])(a)
            return {'ok': True, 'result': self.cv.toExpr(self.finalize(res))}
        except MapleError as exc:
            return {'ok': False, 'error': self.errMsg(short, str(exc))}
        except EngineTimeout:
            return {'ok': False, 'error': '(in {}) computation timed out'.format(short)}
        except Exception as exc:
            if DEBUG:
                traceback.print_exc()
            return {'ok': False, 'error': self.errMsg(short, self.pyMessage(exc))}

    def finalize(self, res):
        if isinstance(res, Seq) and len(res) == 1:
            return res[0]
        return res

    def errMsg(self, name, msg):
        if msg.startswith('(in '):
            return msg
        return '(in {}) {}'.format(name, msg)

    def pyMessage(self, exc):
        if isinstance(exc, ZeroDivisionError):
            return 'numeric exception: division by zero'
        if isinstance(exc, (IndexError, TypeError, AttributeError, KeyError, sp.SympifyError)):
            return 'wrong number (or type) of arguments'
        if isinstance(exc, ValueError) and ('unpack' in str(exc) or 'not enough values' in str(exc)):
            return 'wrong number (or type) of arguments'
        if isinstance(exc, sp.PolynomialError):
            return 'invalid polynomial argument'
        if isinstance(exc, NotImplementedError):
            return 'unable to compute'
        if isinstance(exc, RecursionError):
            return 'expression too deeply nested'
        return '{}'.format(str(exc).split('\n')[0][:200] or type(exc).__name__)



def main():
    real = sys.stdout
    sys.stdout = sys.stderr
    eng = Engine()
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        rid = None
        try:
            req = json.loads(line)
            rid = req.get('id') if isinstance(req, dict) else None
            resp = eng.handle(req)
        except Exception as exc:
            resp = {'ok': False, 'error': str(exc)[:300] or 'invalid request'}
        except EngineTimeout:
            resp = {'ok': False, 'error': 'computation timed out'}
        resp['id'] = rid
        try:
            real.write(json.dumps(resp) + '\n')
        except (TypeError, ValueError):
            real.write(json.dumps({'id': rid, 'ok': False, 'error': 'unserialisable result'}) + '\n')
        real.flush()


if __name__ == '__main__':
    main()
