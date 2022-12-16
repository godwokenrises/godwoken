import { snakeToCamel, camelToSnake } from "../../src/util";
import test from "ava";

test("snakeToCamel", (t) => {
  const obj = {
    hello_world: "hello world",
    hello_earth: {
      hello_human: {
        bob_person: {
          alias_name: "bob",
          age_now: 34,
        },
      },
      hello_cat: {
        bob_cat: {
          alias_name: "bob",
          age_now: 2,
        },
      },
      hello_array: [{ first_array: 1 }],
      hello_nullable: {
        null_val: null,
        nan_val: NaN,
        undefined_val: undefined,
      },
    },
  };
  const expectObj = {
    helloWorld: "hello world",
    helloEarth: {
      helloHuman: {
        bobPerson: {
          aliasName: "bob",
          ageNow: 34,
        },
      },
      helloCat: {
        bobCat: {
          aliasName: "bob",
          ageNow: 2,
        },
      },
      helloArray: [{ firstArray: 1 }],
      helloNullable: {
        nullVal: null,
        nanVal: NaN,
        undefinedVal: undefined,
      },
    },
  };
  t.deepEqual(snakeToCamel(obj), expectObj);
});

test("camelToSnake", (t) => {
  const expectObj = {
    hello_world: "hello world",
    hello_earth: {
      hello_human: {
        bob_person: {
          alias_name: "bob",
          age_now: 34,
        },
      },
      hello_cat: {
        bob_cat: {
          alias_name: "bob",
          age_now: 2,
        },
      },
      hello_array: [{ first_array: 1 }],
      hello_nullable: {
        null_val: null,
        nan_val: NaN,
        undefined_val: undefined,
      },
    },
  };
  const obj = {
    helloWorld: "hello world",
    helloEarth: {
      helloHuman: {
        bobPerson: {
          aliasName: "bob",
          ageNow: 34,
        },
      },
      helloCat: {
        bobCat: {
          aliasName: "bob",
          ageNow: 2,
        },
      },
      helloArray: [{ firstArray: 1 }],
      helloNullable: {
        nullVal: null,
        nanVal: NaN,
        undefinedVal: undefined,
      },
    },
  };
  t.deepEqual(camelToSnake(obj), expectObj);
});

test("overflow", (t) => {
  const overflowDepthObj = {
    hello_world: {
      hello_world: {
        hello_world: {
          hello_world: {
            hello_world: {
              hello_world: {
                hello_world: {
                  hello_world: {
                    hello_world: {
                      hello_world: ["what a small world!"],
                    },
                  },
                },
              },
            },
          },
        },
      },
    },
  };

  const depthObj = {
    hello_world: {
      hello_world: {
        hello_world: {
          hello_world: {
            hello_world: {
              hello_world: {
                hello_world: {
                  hello_world: ["what a small world!"],
                },
              },
            },
          },
        },
      },
    },
  };

  t.throws(() => snakeToCamel(overflowDepthObj), {
    instanceOf: Error,
    message: "[snakeToCamel] recursive depth reached max limit.",
  });
  t.notThrows(() => snakeToCamel(depthObj));
});
