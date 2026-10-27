# read, global counters, if-bodied arrows, elementwise ~, warnlevel, kernelopts
interface(prettyprint = 0, warnlevel = 0):
read "tests/golden/include/harness.mpl":
CHECK("one minus one", 1 - 1);
CHECK("two", 2);
_nok;
KD(1, 1), KD(1, 2);
add(add(KD(i, j), j = 1 .. 3), i = 1 .. 3);
f~([1, 2], [3, 4], z);
(t -> t^2)~([1, 2, 3]);
(t -> 2*t)~(<<1|2>, <3|4>>);
g := proc(n) s := n + 1; s end proc:
g(1);
type(kernelopts(version), string);
try read "tests/golden/include/no_such_file.mpl" catch: printf("caught: %s\n", lastexception[2]) end try;
h := proc() error "bad value %1", 7 end proc:
try h() catch: lastexception[1], lasterror end try;
