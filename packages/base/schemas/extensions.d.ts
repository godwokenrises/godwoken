import {CanCastToArrayBuffer, CreateOptions, OutPoint} from "./godwoken";
export class OutPointVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: Number): OutPoint;
  length(): Number;
}
