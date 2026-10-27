# helper file loaded by the `read` statement in language_read.mpl
_nok := 0:
CHECK := proc(label, e)
  global _nok;
  local z;
  z := e;
  try z := z + 0 catch: end try;
  if z = 0 then
    printf("  [ OK ] %s\n", label);
    _nok := _nok + 1;
  else
    printf("  [FAIL] %s: %a\n", label, z);
  end if;
  NULL;
end proc:
KD := (a, b) -> if a = b then 1 else 0 end if:
printf("harness loaded\n");
