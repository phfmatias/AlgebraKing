# AlgebraKing example: basic algebra (Maple syntax).
restart;
p := (x + 1)^3;
expand(p);
factor(x^3 - 6*x^2 + 11*x - 6);
simplify(sin(x)^2 + cos(x)^2);
normal((x^2 - 1)/(x - 1));
solve(x^2 - 5*x + 6 = 0, x);
solve({2*x + y = 5, x - y = 1}, {x, y});
evalf(sqrt(2));
Digits := 25:
evalf(exp(1));
Digits := 10:
ifactor(360);
isprime(2^31 - 1);
