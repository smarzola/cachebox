const releaseTypes = [
  "feat",
  "fix",
  "perf",
];

const nonReleaseTypes = [
  "docs",
  "test",
  "ci",
  "build",
  "chore",
  "style",
  "refactor",
  "revert",
];

export default {
  extends: ["@commitlint/config-conventional"],
  rules: {
    "type-enum": [2, "always", [...releaseTypes, ...nonReleaseTypes]],
  },
};
