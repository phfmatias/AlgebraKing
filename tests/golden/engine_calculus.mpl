# Calculus and algebra through the SymPy engine.
int(x^2, x);
int(x^2, x = 0 .. 1);
diff(sin(x)^2, x);
diff(x^5, x $ 3);
solve(x^2 - 4 = 0, x);
solve({x + y = 3, x - y = 1}, {x, y});
f := x -> x^3:
D(f)(2);
D(sin);
(D@@2)(sin);
expand((x + 1)^3);
factor(x^2 - 1);
limit(sin(x)/x, x = 0);
eval(diff(y(x), x), x = 0);
with(LinearAlgebra):
Determinant(<<1|2>, <3|4>>);
A := <<2|1>, <1|2>>:
A^(-1);
evalf(Pi, 20);
