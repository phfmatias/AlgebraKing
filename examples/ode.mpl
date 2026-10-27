# AlgebraKing example: differential equations.
restart;
ode := diff(y(x), x, x) + y(x) = 0;
dsolve(ode, y(x));
dsolve({ode, y(0) = 0, D(y)(0) = 1}, y(x));
dsolve(diff(y(x), x) = -2*y(x), y(x));
