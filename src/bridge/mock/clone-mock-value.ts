/** Mock 返回值一律深拷贝，防止调用方修改污染模块级常量或闭包状态。 */
export const cloneMockValue = <T>(value: T): T => structuredClone(value)
