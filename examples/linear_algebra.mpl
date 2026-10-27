# AlgebraKing example: linear algebra with angle-bracket constructors.
restart;
with(LinearAlgebra):
A := <<2|1|0>, <1|3|1>, <0|1|4>>;
b := <1, 2, 3>;
Determinant(A);
A . b;
LinearSolve(A, b);
Transpose(A);
Eigenvalues(<<2|0>, <0|5>>);
A^2;
Rank(A);
