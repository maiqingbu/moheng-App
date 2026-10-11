export function hasKernelStateManageCall(source) {
  return /\bmanage\s*\(\s*KernelState\s*::\s*(?:new|with_[A-Za-z0-9_]+)\s*\(/s.test(source)
}
