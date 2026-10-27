# AlgebraKing example: calculus.
restart;
f := x -> x^2*exp(-x);
D(f);
D(f)(1);
diff(sin(x)*cos(x), x);
int(x*exp(-x), x = 0 .. infinity);
int(1/(x^2 + 1), x);
limit((1 + 1/n)^n, n = infinity);
series(exp(x), x = 0, 5);
sum(1/k^2, k = 1 .. infinity);
taylor(ln(1 + x), x = 0, 4);
