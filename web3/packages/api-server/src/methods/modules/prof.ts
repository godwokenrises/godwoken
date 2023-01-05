import { cpuProf } from "../../decorator";
import v8Profiler from "v8-profiler-next";
import fs from "fs";
import path from "path";

const PROF_TIME_MS = 25000; // 25s

export class Prof {
  constructor() {}

  @cpuProf(PROF_TIME_MS, true)
  async cpu(args: any[]): Promise<string> {
    const fileName = args[args.length - 1];
    return fileName;
  }

  async heap() {
    const createHeadDumpFile = async (fileName: string) => {
      return new Promise((resolve, reject) => {
        const file = fs.createWriteStream(fileName);
        const snapshot = v8Profiler.takeSnapshot();
        const transform = snapshot.export();
        transform.pipe(file);
        transform.on("finish", () => {
          snapshot.delete.bind(snapshot);
          resolve(fileName);
        });
        transform.on("error", reject);
      });
    };
    const fileName = `${Date.now()}.heapsnapshot`;
    await createHeadDumpFile(path.join(fileName));
    return fileName;
  }
}
