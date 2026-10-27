# Language features that need no math engine.
restart;
f := x -> x^2 + 1;
f(3);
g := proc(n) local i, s;
    s := 0;
    for i to n do
        s := s + i^2
    end do;
    s
end proc:
g(10);
L := [seq(k^2, k = 1 .. 6)];
L[2 .. 4];
add(t, t in L);
select(t -> t > 10, L);
map(t -> t/2, L);
x := 'x':
subs(x = 2, x^3 + x);
subs(x = 0, sin(x));
a := b: b := c: c := 7: a;
1/3 + 1/6;
evalf(22/7);
0.1 + 0.2;
A := <<1|2>, <3|4>>;
A . A;
A[2, 1];
V := <1, 2, 3>;
V[3];
T := table():
T[red] := 1: T[blue] := 2:
T[red] + T[blue];
for i from 3 to 1 by -1 do i od;
printf("%d squared is %d\n", 7, 49);
x^2^3;
