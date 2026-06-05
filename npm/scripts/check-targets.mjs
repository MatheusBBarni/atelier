import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import { npmRoot } from "./common.mjs";
import { TARGETS, platformPackageName } from "./targets.mjs";

const require = createRequire(import.meta.url);
const runtimeTargets = require(resolve(npmRoot, "package/lib/targets.cjs"));

export function checkTargets() {
  const failures = [];
  const runtimeKeys = Object.keys(runtimeTargets.TARGETS);
  const sourceKeys = TARGETS.map((target) => target.key);

  if (JSON.stringify(runtimeKeys) !== JSON.stringify(sourceKeys)) {
    failures.push(`runtime target keys ${runtimeKeys.join(", ")} != source target keys ${sourceKeys.join(", ")}`);
  }

  for (const target of TARGETS) {
    const runtimeTarget = runtimeTargets.TARGETS[target.key];
    if (!runtimeTarget) {
      failures.push(`missing runtime target ${target.key}`);
      continue;
    }
    for (const field of ["key", "os", "cpu", "libc", "exe", "archive"]) {
      if ((runtimeTarget[field] ?? undefined) !== (target[field] ?? undefined)) {
        failures.push(`${target.key}.${field} runtime=${runtimeTarget[field]} source=${target[field]}`);
      }
    }
    if (runtimeTarget.packageName !== platformPackageName(target.key)) {
      failures.push(`${target.key}.packageName is ${runtimeTarget.packageName}`);
    }
    const expectedBin = `bin/${target.exe}`;
    if (runtimeTarget.binPath !== expectedBin) {
      failures.push(`${target.key}.binPath is ${runtimeTarget.binPath}, expected ${expectedBin}`);
    }
    const resolvedKey = runtimeTargets.targetKeyFor(target.os, target.cpu, target.libc);
    if (resolvedKey !== target.key) {
      failures.push(`runtime targetKeyFor(${target.os}, ${target.cpu}, ${target.libc}) returned ${resolvedKey}`);
    }
  }

  if (failures.length > 0) {
    throw new Error(failures.join("\n"));
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  checkTargets();
  console.log("runtime target map matches npm/scripts/targets.mjs");
}
