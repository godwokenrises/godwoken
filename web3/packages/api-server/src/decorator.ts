import { asyncSleep } from "./util";
import path from "path";
import fs from "fs";
import v8Profiler from "v8-profiler-next";

export function cpuProf(
  timeMs?: number,
  returnFileName: boolean = false,
  _fileName?: string
) {
  return function (
    _target: any,
    _name: string,
    descriptor: TypedPropertyDescriptor<(args: any[]) => Promise<any>>
  ) {
    const oldFunc = descriptor.value;
    descriptor.value = async function (p: any[]) {
      // set generateType 1 to generate new format for cpuprofile
      // to be compatible with cpuprofile parsing in vscode.
      v8Profiler.setGenerateType(1);
      v8Profiler.startProfiling("CPU profile");

      const fileName = _fileName || `${Date.now()}.cpuprofile`;
      const cpuprofile = path.join(fileName);

      const params = p;
      if (returnFileName === true) {
        params.push(fileName);
      }

      const result = await oldFunc?.apply(this, [params]);

      // stop profile
      if (timeMs != null) {
        await asyncSleep(timeMs);
      }
      const profile = v8Profiler.stopProfiling();
      profile
        .export()
        .pipe(fs.createWriteStream(cpuprofile))
        .on("finish", () => profile.delete());

      return result;
    };
    return descriptor;
  };
}
