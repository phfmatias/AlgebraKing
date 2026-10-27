# engine features needed by Maple 18 verification scripts
interface(prettyprint = 0):
with(LinearAlgebra):
# float matrices: numeric eigen-solver, symmetric case gives real values
ev := Eigenvalues(evalf(Matrix([[1, 1/3], [1/3, 2]]), 30)):
map(t -> evalf(t, 10), convert(ev, list));
Eigenvalues(Matrix([[1., 2], [-3, 1]]));
# Euler identity reached by simplify
simplify(F*cos(w*t) - (F*exp(-I*w*t)/2 + F*exp(I*w*t)/2));
simplify(exp(I*x));
# assuming inside a call applies to the whole argument sequence
f := proc(a, b) [a, b] end proc:
f(1, is(y > 0) assuming y > 1, z > 0);
Re~([1 + 2*I, 3, I]);
# evalf(e, n) and evalf[n](e) evaluate e itself with Digits = n
evalf(1/3 + 0.1, 20);
evalf[20](1/3 + 0.1);
Digits;
KD := (a, b) -> if a = b then 1 else 0 end if:
map(t -> KD(t, 1), [1, 2]);
