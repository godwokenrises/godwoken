import { Cell, Script, helpers } from "@ckb-lumos/base";
const { isCellMatchQueryOptions } = helpers;

interface Options {
  lock?: Script;
}

class CellCollector {
  private options: Options;
  private cells: Cell[];

  constructor(options: Options, cells: Cell[]) {
    this.options = options;
    this.cells = cells;
  }

  async *collect(): AsyncGenerator<Cell> {
    for (const cell of this.cells) {
      if (isCellMatchQueryOptions(cell, this.options)) {
        yield cell;
      }
    }
  }
}

export class CellProvider {
  private cells: Cell[];

  constructor(cells: Cell[]) {
    this.cells = cells;
  }

  collector(options: Options): CellCollector {
    return new CellCollector(options, this.cells);
  }
}
