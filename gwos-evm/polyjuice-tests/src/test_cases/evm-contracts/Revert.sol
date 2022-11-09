pragma solidity ^0.8.4;

contract Revert {
    uint public state = 1;

    function test() external {
        state = 2;
        revert();
    }
}

contract CallRevertWithoutTryCatch {
    uint public state = 1;

    // expected result: state = 1, a.state = 1
    function test(Revert a) external returns (uint) {
        state = 2;
        a.test();
        return 3;
    }
}


contract CallRevertWithTryCatch {
    uint public state = 1;

    // expected result: state = 4, a.state = 1
    function test(Revert a) external returns (uint) {
        state = 2;
        try a.test() {
            state = 3;
            return 5;
        } catch {
            state = 4;
            return 6;
        }
    }
}

contract CallRevertWithTryCatchInDepth {
    uint public state = 1;

    // expected result: state = 3, a.state = 1
    function test(CallRevertWithTryCatch b, Revert a) external returns (uint) {
        state = 2;
        try b.test(a) {
            state = 3;
            return 5;
        } catch {
            state = 4;
            return 6;
        }
    }
}


contract CallRevertWithTryCatchInConstructor {
    uint public state = 1;

    // expected result: state = 4, a.state = 1
    constructor(Revert c){
        state = 2;
        try c.test() {
            state = 3;
        } catch {
            state = 4;
        }
    }
}
