#!/usr/bin/env node
// Jev 시험 협업 도구의 진입점. 사용법과 종료코드는 cli.mjs 머리말과 README.md 에 있다.
import { main } from './cli.mjs';

process.exitCode = await main(process.argv.slice(2));
