# AlgebraKing example: procedures, loops and sequences.
restart;
fib := proc(n::nonnegint) option remember;
    if n < 2 then n else fib(n - 1) + fib(n - 2) end if
end proc:
seq(fib(i), i = 0 .. 15);
collatz := proc(n) local k, m;
    k := 0; m := n;
    while m <> 1 do
        if irem(m, 2) = 0 then m := m/2 else m := 3*m + 1 end if;
        k := k + 1
    end do;
    k
end proc:
collatz(27);
squares := [seq(i^2, i = 1 .. 10)];
add(s, s in squares);
select(isprime, [seq(i, i = 1 .. 30)]);
printf("%d! = %d\n", 20, 20!);
