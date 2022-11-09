pragma solidity >=0.5.12;

contract RecursionContract {
  // naive recursion
  function sum(uint n) external view returns(uint) {
    return n == 0 ? 0 :
      n + this.sum(n-1);
  }

  // pure loop
  function pureSumLoop(uint n) external pure returns(uint) {
    uint256 total = 0;
    for(uint256 i=1; i<=n; i++) {
      total += i;
    }
    return total;
  }

  // tail-recursion
  function sumtailHelper(uint n, uint acc) private view returns(uint) {
      return n == 0 ? acc :
        sumtailHelper(n-1, acc + n);
  }
  function sumTail(uint n) external view returns(uint) {
      return sumtailHelper(n, 0);
  }

  uint x;
  function set(uint y) public {
    x = y;
  }
  function factorial(uint y) internal pure returns(uint) {
    if (y == 1){
        return y;
    } else {
        return y * factorial(y-1);
    }
  }
  function get() public view returns(uint){
    return factorial(x);
  }
}
