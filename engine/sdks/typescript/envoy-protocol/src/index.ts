// @generated - post-processed by build.rs

import * as bare from "@rivetkit/bare-ts"

const DEFAULT_CONFIG = /* @__PURE__ */ bare.Config({})

export type i64 = bigint
export type u16 = number
export type u32 = number
export type u64 = bigint

export type Id = string

export function readId(bc: bare.ByteCursor): Id {
    return bare.readString(bc)
}

export function writeId(bc: bare.ByteCursor, x: Id): void {
    bare.writeString(bc, x)
}

export type Json = string

export function readJson(bc: bare.ByteCursor): Json {
    return bare.readString(bc)
}

export function writeJson(bc: bare.ByteCursor, x: Json): void {
    bare.writeString(bc, x)
}

export type GatewayId = ArrayBuffer

export function readGatewayId(bc: bare.ByteCursor): GatewayId {
    return bare.readFixedData(bc, 4)
}

export function writeGatewayId(bc: bare.ByteCursor, x: GatewayId): void {
    assert(x.byteLength === 4)
    bare.writeFixedData(bc, x)
}

export type RequestId = ArrayBuffer

export function readRequestId(bc: bare.ByteCursor): RequestId {
    return bare.readFixedData(bc, 4)
}

export function writeRequestId(bc: bare.ByteCursor, x: RequestId): void {
    assert(x.byteLength === 4)
    bare.writeFixedData(bc, x)
}

export type MessageIndex = u16

export function readMessageIndex(bc: bare.ByteCursor): MessageIndex {
    return bare.readU16(bc)
}

export function writeMessageIndex(bc: bare.ByteCursor, x: MessageIndex): void {
    bare.writeU16(bc, x)
}

/**
 * Basic types
 */
export type KvKey = ArrayBuffer

export function readKvKey(bc: bare.ByteCursor): KvKey {
    return bare.readData(bc)
}

export function writeKvKey(bc: bare.ByteCursor, x: KvKey): void {
    bare.writeData(bc, x)
}

export type KvValue = ArrayBuffer

export function readKvValue(bc: bare.ByteCursor): KvValue {
    return bare.readData(bc)
}

export function writeKvValue(bc: bare.ByteCursor, x: KvValue): void {
    bare.writeData(bc, x)
}

export type KvMetadata = {
    readonly version: ArrayBuffer
    readonly updateTs: i64
}

export function readKvMetadata(bc: bare.ByteCursor): KvMetadata {
    return {
        version: bare.readData(bc),
        updateTs: bare.readI64(bc),
    }
}

export function writeKvMetadata(bc: bare.ByteCursor, x: KvMetadata): void {
    bare.writeData(bc, x.version)
    bare.writeI64(bc, x.updateTs)
}

/**
 * Query types
 */
export type KvListAllQuery = null

export type KvListRangeQuery = {
    readonly start: KvKey
    readonly end: KvKey
    readonly exclusive: boolean
}

export function readKvListRangeQuery(bc: bare.ByteCursor): KvListRangeQuery {
    return {
        start: readKvKey(bc),
        end: readKvKey(bc),
        exclusive: bare.readBool(bc),
    }
}

export function writeKvListRangeQuery(bc: bare.ByteCursor, x: KvListRangeQuery): void {
    writeKvKey(bc, x.start)
    writeKvKey(bc, x.end)
    bare.writeBool(bc, x.exclusive)
}

export type KvListPrefixQuery = {
    readonly key: KvKey
}

export function readKvListPrefixQuery(bc: bare.ByteCursor): KvListPrefixQuery {
    return {
        key: readKvKey(bc),
    }
}

export function writeKvListPrefixQuery(bc: bare.ByteCursor, x: KvListPrefixQuery): void {
    writeKvKey(bc, x.key)
}

export type KvListQuery =
    | { readonly tag: "KvListAllQuery"; readonly val: KvListAllQuery }
    | { readonly tag: "KvListRangeQuery"; readonly val: KvListRangeQuery }
    | { readonly tag: "KvListPrefixQuery"; readonly val: KvListPrefixQuery }

export function readKvListQuery(bc: bare.ByteCursor): KvListQuery {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "KvListAllQuery", val: null }
        case 1:
            return { tag: "KvListRangeQuery", val: readKvListRangeQuery(bc) }
        case 2:
            return { tag: "KvListPrefixQuery", val: readKvListPrefixQuery(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeKvListQuery(bc: bare.ByteCursor, x: KvListQuery): void {
    switch (x.tag) {
        case "KvListAllQuery": {
            bare.writeU8(bc, 0)
            break
        }
        case "KvListRangeQuery": {
            bare.writeU8(bc, 1)
            writeKvListRangeQuery(bc, x.val)
            break
        }
        case "KvListPrefixQuery": {
            bare.writeU8(bc, 2)
            writeKvListPrefixQuery(bc, x.val)
            break
        }
    }
}

function read0(bc: bare.ByteCursor): readonly KvKey[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readKvKey(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readKvKey(bc)
    }
    return result
}

function write0(bc: bare.ByteCursor, x: readonly KvKey[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeKvKey(bc, x[i])
    }
}

/**
 * Request types
 */
export type KvGetRequest = {
    readonly keys: readonly KvKey[]
}

export function readKvGetRequest(bc: bare.ByteCursor): KvGetRequest {
    return {
        keys: read0(bc),
    }
}

export function writeKvGetRequest(bc: bare.ByteCursor, x: KvGetRequest): void {
    write0(bc, x.keys)
}

function read1(bc: bare.ByteCursor): boolean | null {
    return bare.readBool(bc) ? bare.readBool(bc) : null
}

function write1(bc: bare.ByteCursor, x: boolean | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeBool(bc, x)
    }
}

function read2(bc: bare.ByteCursor): u64 | null {
    return bare.readBool(bc) ? bare.readU64(bc) : null
}

function write2(bc: bare.ByteCursor, x: u64 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeU64(bc, x)
    }
}

export type KvListRequest = {
    readonly query: KvListQuery
    readonly reverse: boolean | null
    readonly limit: u64 | null
}

export function readKvListRequest(bc: bare.ByteCursor): KvListRequest {
    return {
        query: readKvListQuery(bc),
        reverse: read1(bc),
        limit: read2(bc),
    }
}

export function writeKvListRequest(bc: bare.ByteCursor, x: KvListRequest): void {
    writeKvListQuery(bc, x.query)
    write1(bc, x.reverse)
    write2(bc, x.limit)
}

function read3(bc: bare.ByteCursor): readonly KvValue[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readKvValue(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readKvValue(bc)
    }
    return result
}

function write3(bc: bare.ByteCursor, x: readonly KvValue[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeKvValue(bc, x[i])
    }
}

export type KvPutRequest = {
    readonly keys: readonly KvKey[]
    readonly values: readonly KvValue[]
}

export function readKvPutRequest(bc: bare.ByteCursor): KvPutRequest {
    return {
        keys: read0(bc),
        values: read3(bc),
    }
}

export function writeKvPutRequest(bc: bare.ByteCursor, x: KvPutRequest): void {
    write0(bc, x.keys)
    write3(bc, x.values)
}

export type KvDeleteRequest = {
    readonly keys: readonly KvKey[]
}

export function readKvDeleteRequest(bc: bare.ByteCursor): KvDeleteRequest {
    return {
        keys: read0(bc),
    }
}

export function writeKvDeleteRequest(bc: bare.ByteCursor, x: KvDeleteRequest): void {
    write0(bc, x.keys)
}

export type KvDeleteRangeRequest = {
    readonly start: KvKey
    readonly end: KvKey
}

export function readKvDeleteRangeRequest(bc: bare.ByteCursor): KvDeleteRangeRequest {
    return {
        start: readKvKey(bc),
        end: readKvKey(bc),
    }
}

export function writeKvDeleteRangeRequest(bc: bare.ByteCursor, x: KvDeleteRangeRequest): void {
    writeKvKey(bc, x.start)
    writeKvKey(bc, x.end)
}

export type KvDropRequest = null

/**
 * Response types
 */
export type KvErrorResponse = {
    readonly message: string
}

export function readKvErrorResponse(bc: bare.ByteCursor): KvErrorResponse {
    return {
        message: bare.readString(bc),
    }
}

export function writeKvErrorResponse(bc: bare.ByteCursor, x: KvErrorResponse): void {
    bare.writeString(bc, x.message)
}

function read4(bc: bare.ByteCursor): readonly KvMetadata[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readKvMetadata(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readKvMetadata(bc)
    }
    return result
}

function write4(bc: bare.ByteCursor, x: readonly KvMetadata[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeKvMetadata(bc, x[i])
    }
}

export type KvGetResponse = {
    readonly keys: readonly KvKey[]
    readonly values: readonly KvValue[]
    readonly metadata: readonly KvMetadata[]
}

export function readKvGetResponse(bc: bare.ByteCursor): KvGetResponse {
    return {
        keys: read0(bc),
        values: read3(bc),
        metadata: read4(bc),
    }
}

export function writeKvGetResponse(bc: bare.ByteCursor, x: KvGetResponse): void {
    write0(bc, x.keys)
    write3(bc, x.values)
    write4(bc, x.metadata)
}

export type KvListResponse = {
    readonly keys: readonly KvKey[]
    readonly values: readonly KvValue[]
    readonly metadata: readonly KvMetadata[]
}

export function readKvListResponse(bc: bare.ByteCursor): KvListResponse {
    return {
        keys: read0(bc),
        values: read3(bc),
        metadata: read4(bc),
    }
}

export function writeKvListResponse(bc: bare.ByteCursor, x: KvListResponse): void {
    write0(bc, x.keys)
    write3(bc, x.values)
    write4(bc, x.metadata)
}

export type KvPutResponse = null

export type KvDeleteResponse = null

export type KvDropResponse = null

/**
 * Request/Response unions
 */
export type KvRequestData =
    | { readonly tag: "KvGetRequest"; readonly val: KvGetRequest }
    | { readonly tag: "KvListRequest"; readonly val: KvListRequest }
    | { readonly tag: "KvPutRequest"; readonly val: KvPutRequest }
    | { readonly tag: "KvDeleteRequest"; readonly val: KvDeleteRequest }
    | { readonly tag: "KvDeleteRangeRequest"; readonly val: KvDeleteRangeRequest }
    | { readonly tag: "KvDropRequest"; readonly val: KvDropRequest }

export function readKvRequestData(bc: bare.ByteCursor): KvRequestData {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "KvGetRequest", val: readKvGetRequest(bc) }
        case 1:
            return { tag: "KvListRequest", val: readKvListRequest(bc) }
        case 2:
            return { tag: "KvPutRequest", val: readKvPutRequest(bc) }
        case 3:
            return { tag: "KvDeleteRequest", val: readKvDeleteRequest(bc) }
        case 4:
            return { tag: "KvDeleteRangeRequest", val: readKvDeleteRangeRequest(bc) }
        case 5:
            return { tag: "KvDropRequest", val: null }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeKvRequestData(bc: bare.ByteCursor, x: KvRequestData): void {
    switch (x.tag) {
        case "KvGetRequest": {
            bare.writeU8(bc, 0)
            writeKvGetRequest(bc, x.val)
            break
        }
        case "KvListRequest": {
            bare.writeU8(bc, 1)
            writeKvListRequest(bc, x.val)
            break
        }
        case "KvPutRequest": {
            bare.writeU8(bc, 2)
            writeKvPutRequest(bc, x.val)
            break
        }
        case "KvDeleteRequest": {
            bare.writeU8(bc, 3)
            writeKvDeleteRequest(bc, x.val)
            break
        }
        case "KvDeleteRangeRequest": {
            bare.writeU8(bc, 4)
            writeKvDeleteRangeRequest(bc, x.val)
            break
        }
        case "KvDropRequest": {
            bare.writeU8(bc, 5)
            break
        }
    }
}

export type KvResponseData =
    | { readonly tag: "KvErrorResponse"; readonly val: KvErrorResponse }
    | { readonly tag: "KvGetResponse"; readonly val: KvGetResponse }
    | { readonly tag: "KvListResponse"; readonly val: KvListResponse }
    | { readonly tag: "KvPutResponse"; readonly val: KvPutResponse }
    | { readonly tag: "KvDeleteResponse"; readonly val: KvDeleteResponse }
    | { readonly tag: "KvDropResponse"; readonly val: KvDropResponse }

export function readKvResponseData(bc: bare.ByteCursor): KvResponseData {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "KvErrorResponse", val: readKvErrorResponse(bc) }
        case 1:
            return { tag: "KvGetResponse", val: readKvGetResponse(bc) }
        case 2:
            return { tag: "KvListResponse", val: readKvListResponse(bc) }
        case 3:
            return { tag: "KvPutResponse", val: null }
        case 4:
            return { tag: "KvDeleteResponse", val: null }
        case 5:
            return { tag: "KvDropResponse", val: null }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeKvResponseData(bc: bare.ByteCursor, x: KvResponseData): void {
    switch (x.tag) {
        case "KvErrorResponse": {
            bare.writeU8(bc, 0)
            writeKvErrorResponse(bc, x.val)
            break
        }
        case "KvGetResponse": {
            bare.writeU8(bc, 1)
            writeKvGetResponse(bc, x.val)
            break
        }
        case "KvListResponse": {
            bare.writeU8(bc, 2)
            writeKvListResponse(bc, x.val)
            break
        }
        case "KvPutResponse": {
            bare.writeU8(bc, 3)
            break
        }
        case "KvDeleteResponse": {
            bare.writeU8(bc, 4)
            break
        }
        case "KvDropResponse": {
            bare.writeU8(bc, 5)
            break
        }
    }
}

export type SqlitePgno = u32

export function readSqlitePgno(bc: bare.ByteCursor): SqlitePgno {
    return bare.readU32(bc)
}

export function writeSqlitePgno(bc: bare.ByteCursor, x: SqlitePgno): void {
    bare.writeU32(bc, x)
}

export type SqliteGeneration = u64

export function readSqliteGeneration(bc: bare.ByteCursor): SqliteGeneration {
    return bare.readU64(bc)
}

export function writeSqliteGeneration(bc: bare.ByteCursor, x: SqliteGeneration): void {
    bare.writeU64(bc, x)
}

export type SqlitePageBytes = ArrayBuffer

export function readSqlitePageBytes(bc: bare.ByteCursor): SqlitePageBytes {
    return bare.readData(bc)
}

export function writeSqlitePageBytes(bc: bare.ByteCursor, x: SqlitePageBytes): void {
    bare.writeData(bc, x)
}

export type SqliteDirtyPage = {
    readonly pgno: SqlitePgno
    readonly bytes: SqlitePageBytes
}

export function readSqliteDirtyPage(bc: bare.ByteCursor): SqliteDirtyPage {
    return {
        pgno: readSqlitePgno(bc),
        bytes: readSqlitePageBytes(bc),
    }
}

export function writeSqliteDirtyPage(bc: bare.ByteCursor, x: SqliteDirtyPage): void {
    writeSqlitePgno(bc, x.pgno)
    writeSqlitePageBytes(bc, x.bytes)
}

function read5(bc: bare.ByteCursor): SqlitePageBytes | null {
    return bare.readBool(bc) ? readSqlitePageBytes(bc) : null
}

function write5(bc: bare.ByteCursor, x: SqlitePageBytes | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeSqlitePageBytes(bc, x)
    }
}

export type SqliteFetchedPage = {
    readonly pgno: SqlitePgno
    readonly bytes: SqlitePageBytes | null
}

export function readSqliteFetchedPage(bc: bare.ByteCursor): SqliteFetchedPage {
    return {
        pgno: readSqlitePgno(bc),
        bytes: read5(bc),
    }
}

export function writeSqliteFetchedPage(bc: bare.ByteCursor, x: SqliteFetchedPage): void {
    writeSqlitePgno(bc, x.pgno)
    write5(bc, x.bytes)
}

function read6(bc: bare.ByteCursor): readonly SqlitePgno[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readSqlitePgno(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readSqlitePgno(bc)
    }
    return result
}

function write6(bc: bare.ByteCursor, x: readonly SqlitePgno[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeSqlitePgno(bc, x[i])
    }
}

export type SqliteGetPagesRequest = {
    readonly actorId: Id
    readonly pgnos: readonly SqlitePgno[]
    readonly expectedGeneration: u64 | null
    readonly expectedHeadTxid: u64 | null
}

export function readSqliteGetPagesRequest(bc: bare.ByteCursor): SqliteGetPagesRequest {
    return {
        actorId: readId(bc),
        pgnos: read6(bc),
        expectedGeneration: read2(bc),
        expectedHeadTxid: read2(bc),
    }
}

export function writeSqliteGetPagesRequest(bc: bare.ByteCursor, x: SqliteGetPagesRequest): void {
    writeId(bc, x.actorId)
    write6(bc, x.pgnos)
    write2(bc, x.expectedGeneration)
    write2(bc, x.expectedHeadTxid)
}

function read7(bc: bare.ByteCursor): readonly SqliteFetchedPage[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readSqliteFetchedPage(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readSqliteFetchedPage(bc)
    }
    return result
}

function write7(bc: bare.ByteCursor, x: readonly SqliteFetchedPage[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeSqliteFetchedPage(bc, x[i])
    }
}

export type SqliteGetPagesOk = {
    readonly pages: readonly SqliteFetchedPage[]
    readonly headTxid: u64 | null
}

export function readSqliteGetPagesOk(bc: bare.ByteCursor): SqliteGetPagesOk {
    return {
        pages: read7(bc),
        headTxid: read2(bc),
    }
}

export function writeSqliteGetPagesOk(bc: bare.ByteCursor, x: SqliteGetPagesOk): void {
    write7(bc, x.pages)
    write2(bc, x.headTxid)
}

export type SqliteErrorResponse = {
    readonly group: string
    readonly code: string
    readonly message: string
}

export function readSqliteErrorResponse(bc: bare.ByteCursor): SqliteErrorResponse {
    return {
        group: bare.readString(bc),
        code: bare.readString(bc),
        message: bare.readString(bc),
    }
}

export function writeSqliteErrorResponse(bc: bare.ByteCursor, x: SqliteErrorResponse): void {
    bare.writeString(bc, x.group)
    bare.writeString(bc, x.code)
    bare.writeString(bc, x.message)
}

export type SqliteGetPagesResponse =
    | { readonly tag: "SqliteGetPagesOk"; readonly val: SqliteGetPagesOk }
    | { readonly tag: "SqliteErrorResponse"; readonly val: SqliteErrorResponse }

export function readSqliteGetPagesResponse(bc: bare.ByteCursor): SqliteGetPagesResponse {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "SqliteGetPagesOk", val: readSqliteGetPagesOk(bc) }
        case 1:
            return { tag: "SqliteErrorResponse", val: readSqliteErrorResponse(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeSqliteGetPagesResponse(bc: bare.ByteCursor, x: SqliteGetPagesResponse): void {
    switch (x.tag) {
        case "SqliteGetPagesOk": {
            bare.writeU8(bc, 0)
            writeSqliteGetPagesOk(bc, x.val)
            break
        }
        case "SqliteErrorResponse": {
            bare.writeU8(bc, 1)
            writeSqliteErrorResponse(bc, x.val)
            break
        }
    }
}

function read8(bc: bare.ByteCursor): readonly SqliteDirtyPage[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readSqliteDirtyPage(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readSqliteDirtyPage(bc)
    }
    return result
}

function write8(bc: bare.ByteCursor, x: readonly SqliteDirtyPage[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeSqliteDirtyPage(bc, x[i])
    }
}

export type SqliteCommitRequest = {
    readonly actorId: Id
    readonly dirtyPages: readonly SqliteDirtyPage[]
    readonly dbSizePages: u32
    readonly nowMs: i64
    readonly expectedGeneration: u64 | null
    readonly expectedHeadTxid: u64 | null
}

export function readSqliteCommitRequest(bc: bare.ByteCursor): SqliteCommitRequest {
    return {
        actorId: readId(bc),
        dirtyPages: read8(bc),
        dbSizePages: bare.readU32(bc),
        nowMs: bare.readI64(bc),
        expectedGeneration: read2(bc),
        expectedHeadTxid: read2(bc),
    }
}

export function writeSqliteCommitRequest(bc: bare.ByteCursor, x: SqliteCommitRequest): void {
    writeId(bc, x.actorId)
    write8(bc, x.dirtyPages)
    bare.writeU32(bc, x.dbSizePages)
    bare.writeI64(bc, x.nowMs)
    write2(bc, x.expectedGeneration)
    write2(bc, x.expectedHeadTxid)
}

export type SqliteCommitOk = {
    readonly headTxid: u64 | null
}

export function readSqliteCommitOk(bc: bare.ByteCursor): SqliteCommitOk {
    return {
        headTxid: read2(bc),
    }
}

export function writeSqliteCommitOk(bc: bare.ByteCursor, x: SqliteCommitOk): void {
    write2(bc, x.headTxid)
}

export type SqliteCommitResponse =
    | { readonly tag: "SqliteCommitOk"; readonly val: SqliteCommitOk }
    | { readonly tag: "SqliteErrorResponse"; readonly val: SqliteErrorResponse }

export function readSqliteCommitResponse(bc: bare.ByteCursor): SqliteCommitResponse {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "SqliteCommitOk", val: readSqliteCommitOk(bc) }
        case 1:
            return { tag: "SqliteErrorResponse", val: readSqliteErrorResponse(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeSqliteCommitResponse(bc: bare.ByteCursor, x: SqliteCommitResponse): void {
    switch (x.tag) {
        case "SqliteCommitOk": {
            bare.writeU8(bc, 0)
            writeSqliteCommitOk(bc, x.val)
            break
        }
        case "SqliteErrorResponse": {
            bare.writeU8(bc, 1)
            writeSqliteErrorResponse(bc, x.val)
            break
        }
    }
}

export type SqliteValueNull = null

export type SqliteValueInteger = {
    readonly value: i64
}

export function readSqliteValueInteger(bc: bare.ByteCursor): SqliteValueInteger {
    return {
        value: bare.readI64(bc),
    }
}

export function writeSqliteValueInteger(bc: bare.ByteCursor, x: SqliteValueInteger): void {
    bare.writeI64(bc, x.value)
}

export type SqliteValueFloat = {
    readonly value: ArrayBuffer
}

export function readSqliteValueFloat(bc: bare.ByteCursor): SqliteValueFloat {
    return {
        value: bare.readFixedData(bc, 8),
    }
}

export function writeSqliteValueFloat(bc: bare.ByteCursor, x: SqliteValueFloat): void {
    {
        assert(x.value.byteLength === 8)
        bare.writeFixedData(bc, x.value)
    }
}

export type SqliteValueText = {
    readonly value: string
}

export function readSqliteValueText(bc: bare.ByteCursor): SqliteValueText {
    return {
        value: bare.readString(bc),
    }
}

export function writeSqliteValueText(bc: bare.ByteCursor, x: SqliteValueText): void {
    bare.writeString(bc, x.value)
}

export type SqliteValueBlob = {
    readonly value: ArrayBuffer
}

export function readSqliteValueBlob(bc: bare.ByteCursor): SqliteValueBlob {
    return {
        value: bare.readData(bc),
    }
}

export function writeSqliteValueBlob(bc: bare.ByteCursor, x: SqliteValueBlob): void {
    bare.writeData(bc, x.value)
}

export type SqliteBindParam =
    | { readonly tag: "SqliteValueNull"; readonly val: SqliteValueNull }
    | { readonly tag: "SqliteValueInteger"; readonly val: SqliteValueInteger }
    | { readonly tag: "SqliteValueFloat"; readonly val: SqliteValueFloat }
    | { readonly tag: "SqliteValueText"; readonly val: SqliteValueText }
    | { readonly tag: "SqliteValueBlob"; readonly val: SqliteValueBlob }

export function readSqliteBindParam(bc: bare.ByteCursor): SqliteBindParam {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "SqliteValueNull", val: null }
        case 1:
            return { tag: "SqliteValueInteger", val: readSqliteValueInteger(bc) }
        case 2:
            return { tag: "SqliteValueFloat", val: readSqliteValueFloat(bc) }
        case 3:
            return { tag: "SqliteValueText", val: readSqliteValueText(bc) }
        case 4:
            return { tag: "SqliteValueBlob", val: readSqliteValueBlob(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeSqliteBindParam(bc: bare.ByteCursor, x: SqliteBindParam): void {
    switch (x.tag) {
        case "SqliteValueNull": {
            bare.writeU8(bc, 0)
            break
        }
        case "SqliteValueInteger": {
            bare.writeU8(bc, 1)
            writeSqliteValueInteger(bc, x.val)
            break
        }
        case "SqliteValueFloat": {
            bare.writeU8(bc, 2)
            writeSqliteValueFloat(bc, x.val)
            break
        }
        case "SqliteValueText": {
            bare.writeU8(bc, 3)
            writeSqliteValueText(bc, x.val)
            break
        }
        case "SqliteValueBlob": {
            bare.writeU8(bc, 4)
            writeSqliteValueBlob(bc, x.val)
            break
        }
    }
}

export type SqliteColumnValue =
    | { readonly tag: "SqliteValueNull"; readonly val: SqliteValueNull }
    | { readonly tag: "SqliteValueInteger"; readonly val: SqliteValueInteger }
    | { readonly tag: "SqliteValueFloat"; readonly val: SqliteValueFloat }
    | { readonly tag: "SqliteValueText"; readonly val: SqliteValueText }
    | { readonly tag: "SqliteValueBlob"; readonly val: SqliteValueBlob }

export function readSqliteColumnValue(bc: bare.ByteCursor): SqliteColumnValue {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "SqliteValueNull", val: null }
        case 1:
            return { tag: "SqliteValueInteger", val: readSqliteValueInteger(bc) }
        case 2:
            return { tag: "SqliteValueFloat", val: readSqliteValueFloat(bc) }
        case 3:
            return { tag: "SqliteValueText", val: readSqliteValueText(bc) }
        case 4:
            return { tag: "SqliteValueBlob", val: readSqliteValueBlob(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeSqliteColumnValue(bc: bare.ByteCursor, x: SqliteColumnValue): void {
    switch (x.tag) {
        case "SqliteValueNull": {
            bare.writeU8(bc, 0)
            break
        }
        case "SqliteValueInteger": {
            bare.writeU8(bc, 1)
            writeSqliteValueInteger(bc, x.val)
            break
        }
        case "SqliteValueFloat": {
            bare.writeU8(bc, 2)
            writeSqliteValueFloat(bc, x.val)
            break
        }
        case "SqliteValueText": {
            bare.writeU8(bc, 3)
            writeSqliteValueText(bc, x.val)
            break
        }
        case "SqliteValueBlob": {
            bare.writeU8(bc, 4)
            writeSqliteValueBlob(bc, x.val)
            break
        }
    }
}

function read9(bc: bare.ByteCursor): readonly string[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [bare.readString(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = bare.readString(bc)
    }
    return result
}

function write9(bc: bare.ByteCursor, x: readonly string[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        bare.writeString(bc, x[i])
    }
}

function read10(bc: bare.ByteCursor): readonly SqliteColumnValue[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readSqliteColumnValue(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readSqliteColumnValue(bc)
    }
    return result
}

function write10(bc: bare.ByteCursor, x: readonly SqliteColumnValue[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeSqliteColumnValue(bc, x[i])
    }
}

function read11(bc: bare.ByteCursor): readonly (readonly SqliteColumnValue[])[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [read10(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = read10(bc)
    }
    return result
}

function write11(bc: bare.ByteCursor, x: readonly (readonly SqliteColumnValue[])[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        write10(bc, x[i])
    }
}

export type SqliteQueryResult = {
    readonly columns: readonly string[]
    readonly rows: readonly (readonly SqliteColumnValue[])[]
}

export function readSqliteQueryResult(bc: bare.ByteCursor): SqliteQueryResult {
    return {
        columns: read9(bc),
        rows: read11(bc),
    }
}

export function writeSqliteQueryResult(bc: bare.ByteCursor, x: SqliteQueryResult): void {
    write9(bc, x.columns)
    write11(bc, x.rows)
}

function read12(bc: bare.ByteCursor): i64 | null {
    return bare.readBool(bc) ? bare.readI64(bc) : null
}

function write12(bc: bare.ByteCursor, x: i64 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeI64(bc, x)
    }
}

export type SqliteExecuteResult = {
    readonly columns: readonly string[]
    readonly rows: readonly (readonly SqliteColumnValue[])[]
    readonly changes: i64
    readonly lastInsertRowId: i64 | null
}

export function readSqliteExecuteResult(bc: bare.ByteCursor): SqliteExecuteResult {
    return {
        columns: read9(bc),
        rows: read11(bc),
        changes: bare.readI64(bc),
        lastInsertRowId: read12(bc),
    }
}

export function writeSqliteExecuteResult(bc: bare.ByteCursor, x: SqliteExecuteResult): void {
    write9(bc, x.columns)
    write11(bc, x.rows)
    bare.writeI64(bc, x.changes)
    write12(bc, x.lastInsertRowId)
}

export type SqliteExecRequest = {
    readonly namespaceId: Id
    readonly actorId: Id
    readonly generation: SqliteGeneration
    readonly sql: string
}

export function readSqliteExecRequest(bc: bare.ByteCursor): SqliteExecRequest {
    return {
        namespaceId: readId(bc),
        actorId: readId(bc),
        generation: readSqliteGeneration(bc),
        sql: bare.readString(bc),
    }
}

export function writeSqliteExecRequest(bc: bare.ByteCursor, x: SqliteExecRequest): void {
    writeId(bc, x.namespaceId)
    writeId(bc, x.actorId)
    writeSqliteGeneration(bc, x.generation)
    bare.writeString(bc, x.sql)
}

function read13(bc: bare.ByteCursor): readonly SqliteBindParam[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readSqliteBindParam(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readSqliteBindParam(bc)
    }
    return result
}

function write13(bc: bare.ByteCursor, x: readonly SqliteBindParam[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeSqliteBindParam(bc, x[i])
    }
}

function read14(bc: bare.ByteCursor): readonly SqliteBindParam[] | null {
    return bare.readBool(bc) ? read13(bc) : null
}

function write14(bc: bare.ByteCursor, x: readonly SqliteBindParam[] | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        write13(bc, x)
    }
}

export type SqliteExecuteRequest = {
    readonly namespaceId: Id
    readonly actorId: Id
    readonly generation: SqliteGeneration
    readonly sql: string
    readonly params: readonly SqliteBindParam[] | null
}

export function readSqliteExecuteRequest(bc: bare.ByteCursor): SqliteExecuteRequest {
    return {
        namespaceId: readId(bc),
        actorId: readId(bc),
        generation: readSqliteGeneration(bc),
        sql: bare.readString(bc),
        params: read14(bc),
    }
}

export function writeSqliteExecuteRequest(bc: bare.ByteCursor, x: SqliteExecuteRequest): void {
    writeId(bc, x.namespaceId)
    writeId(bc, x.actorId)
    writeSqliteGeneration(bc, x.generation)
    bare.writeString(bc, x.sql)
    write14(bc, x.params)
}

export type SqliteBatchStatement = {
    readonly sql: string
    readonly params: readonly SqliteBindParam[] | null
}

export function readSqliteBatchStatement(bc: bare.ByteCursor): SqliteBatchStatement {
    return {
        sql: bare.readString(bc),
        params: read14(bc),
    }
}

export function writeSqliteBatchStatement(bc: bare.ByteCursor, x: SqliteBatchStatement): void {
    bare.writeString(bc, x.sql)
    write14(bc, x.params)
}

function read15(bc: bare.ByteCursor): readonly SqliteBatchStatement[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readSqliteBatchStatement(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readSqliteBatchStatement(bc)
    }
    return result
}

function write15(bc: bare.ByteCursor, x: readonly SqliteBatchStatement[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeSqliteBatchStatement(bc, x[i])
    }
}

export type SqliteExecuteBatchRequest = {
    readonly namespaceId: Id
    readonly actorId: Id
    readonly generation: SqliteGeneration
    readonly statements: readonly SqliteBatchStatement[]
}

export function readSqliteExecuteBatchRequest(bc: bare.ByteCursor): SqliteExecuteBatchRequest {
    return {
        namespaceId: readId(bc),
        actorId: readId(bc),
        generation: readSqliteGeneration(bc),
        statements: read15(bc),
    }
}

export function writeSqliteExecuteBatchRequest(bc: bare.ByteCursor, x: SqliteExecuteBatchRequest): void {
    writeId(bc, x.namespaceId)
    writeId(bc, x.actorId)
    writeSqliteGeneration(bc, x.generation)
    write15(bc, x.statements)
}

export type SqliteExecOk = {
    readonly result: SqliteQueryResult
}

export function readSqliteExecOk(bc: bare.ByteCursor): SqliteExecOk {
    return {
        result: readSqliteQueryResult(bc),
    }
}

export function writeSqliteExecOk(bc: bare.ByteCursor, x: SqliteExecOk): void {
    writeSqliteQueryResult(bc, x.result)
}

export type SqliteExecuteOk = {
    readonly result: SqliteExecuteResult
}

export function readSqliteExecuteOk(bc: bare.ByteCursor): SqliteExecuteOk {
    return {
        result: readSqliteExecuteResult(bc),
    }
}

export function writeSqliteExecuteOk(bc: bare.ByteCursor, x: SqliteExecuteOk): void {
    writeSqliteExecuteResult(bc, x.result)
}

function read16(bc: bare.ByteCursor): readonly SqliteExecuteResult[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readSqliteExecuteResult(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readSqliteExecuteResult(bc)
    }
    return result
}

function write16(bc: bare.ByteCursor, x: readonly SqliteExecuteResult[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeSqliteExecuteResult(bc, x[i])
    }
}

export type SqliteExecuteBatchOk = {
    readonly results: readonly SqliteExecuteResult[]
}

export function readSqliteExecuteBatchOk(bc: bare.ByteCursor): SqliteExecuteBatchOk {
    return {
        results: read16(bc),
    }
}

export function writeSqliteExecuteBatchOk(bc: bare.ByteCursor, x: SqliteExecuteBatchOk): void {
    write16(bc, x.results)
}

export type SqliteExecResponse =
    | { readonly tag: "SqliteExecOk"; readonly val: SqliteExecOk }
    | { readonly tag: "SqliteErrorResponse"; readonly val: SqliteErrorResponse }

export function readSqliteExecResponse(bc: bare.ByteCursor): SqliteExecResponse {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "SqliteExecOk", val: readSqliteExecOk(bc) }
        case 1:
            return { tag: "SqliteErrorResponse", val: readSqliteErrorResponse(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeSqliteExecResponse(bc: bare.ByteCursor, x: SqliteExecResponse): void {
    switch (x.tag) {
        case "SqliteExecOk": {
            bare.writeU8(bc, 0)
            writeSqliteExecOk(bc, x.val)
            break
        }
        case "SqliteErrorResponse": {
            bare.writeU8(bc, 1)
            writeSqliteErrorResponse(bc, x.val)
            break
        }
    }
}

export type SqliteExecuteResponse =
    | { readonly tag: "SqliteExecuteOk"; readonly val: SqliteExecuteOk }
    | { readonly tag: "SqliteErrorResponse"; readonly val: SqliteErrorResponse }

export function readSqliteExecuteResponse(bc: bare.ByteCursor): SqliteExecuteResponse {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "SqliteExecuteOk", val: readSqliteExecuteOk(bc) }
        case 1:
            return { tag: "SqliteErrorResponse", val: readSqliteErrorResponse(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeSqliteExecuteResponse(bc: bare.ByteCursor, x: SqliteExecuteResponse): void {
    switch (x.tag) {
        case "SqliteExecuteOk": {
            bare.writeU8(bc, 0)
            writeSqliteExecuteOk(bc, x.val)
            break
        }
        case "SqliteErrorResponse": {
            bare.writeU8(bc, 1)
            writeSqliteErrorResponse(bc, x.val)
            break
        }
    }
}

export type SqliteExecuteBatchResponse =
    | { readonly tag: "SqliteExecuteBatchOk"; readonly val: SqliteExecuteBatchOk }
    | { readonly tag: "SqliteErrorResponse"; readonly val: SqliteErrorResponse }

export function readSqliteExecuteBatchResponse(bc: bare.ByteCursor): SqliteExecuteBatchResponse {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "SqliteExecuteBatchOk", val: readSqliteExecuteBatchOk(bc) }
        case 1:
            return { tag: "SqliteErrorResponse", val: readSqliteErrorResponse(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeSqliteExecuteBatchResponse(bc: bare.ByteCursor, x: SqliteExecuteBatchResponse): void {
    switch (x.tag) {
        case "SqliteExecuteBatchOk": {
            bare.writeU8(bc, 0)
            writeSqliteExecuteBatchOk(bc, x.val)
            break
        }
        case "SqliteErrorResponse": {
            bare.writeU8(bc, 1)
            writeSqliteErrorResponse(bc, x.val)
            break
        }
    }
}

/**
 * Core
 */
export enum StopCode {
    Ok = "Ok",
    Error = "Error",
}

export function readStopCode(bc: bare.ByteCursor): StopCode {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return StopCode.Ok
        case 1:
            return StopCode.Error
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeStopCode(bc: bare.ByteCursor, x: StopCode): void {
    switch (x) {
        case StopCode.Ok: {
            bare.writeU8(bc, 0)
            break
        }
        case StopCode.Error: {
            bare.writeU8(bc, 1)
            break
        }
    }
}

export type ActorName = {
    readonly metadata: Json
}

export function readActorName(bc: bare.ByteCursor): ActorName {
    return {
        metadata: readJson(bc),
    }
}

export function writeActorName(bc: bare.ByteCursor, x: ActorName): void {
    writeJson(bc, x.metadata)
}

function read17(bc: bare.ByteCursor): string | null {
    return bare.readBool(bc) ? bare.readString(bc) : null
}

function write17(bc: bare.ByteCursor, x: string | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeString(bc, x)
    }
}

function read18(bc: bare.ByteCursor): ArrayBuffer | null {
    return bare.readBool(bc) ? bare.readData(bc) : null
}

function write18(bc: bare.ByteCursor, x: ArrayBuffer | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeData(bc, x)
    }
}

export type ActorConfig = {
    readonly name: string
    readonly key: string | null
    readonly createTs: i64
    readonly input: ArrayBuffer | null
}

export function readActorConfig(bc: bare.ByteCursor): ActorConfig {
    return {
        name: bare.readString(bc),
        key: read17(bc),
        createTs: bare.readI64(bc),
        input: read18(bc),
    }
}

export function writeActorConfig(bc: bare.ByteCursor, x: ActorConfig): void {
    bare.writeString(bc, x.name)
    write17(bc, x.key)
    bare.writeI64(bc, x.createTs)
    write18(bc, x.input)
}

export type ActorCheckpoint = {
    readonly actorId: Id
    readonly generation: u32
    readonly index: i64
}

export function readActorCheckpoint(bc: bare.ByteCursor): ActorCheckpoint {
    return {
        actorId: readId(bc),
        generation: bare.readU32(bc),
        index: bare.readI64(bc),
    }
}

export function writeActorCheckpoint(bc: bare.ByteCursor, x: ActorCheckpoint): void {
    writeId(bc, x.actorId)
    bare.writeU32(bc, x.generation)
    bare.writeI64(bc, x.index)
}

/**
 * Intent
 */
export type ActorIntentSleep = null

export type ActorIntentStop = null

export type ActorIntent =
    | { readonly tag: "ActorIntentSleep"; readonly val: ActorIntentSleep }
    | { readonly tag: "ActorIntentStop"; readonly val: ActorIntentStop }

export function readActorIntent(bc: bare.ByteCursor): ActorIntent {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "ActorIntentSleep", val: null }
        case 1:
            return { tag: "ActorIntentStop", val: null }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeActorIntent(bc: bare.ByteCursor, x: ActorIntent): void {
    switch (x.tag) {
        case "ActorIntentSleep": {
            bare.writeU8(bc, 0)
            break
        }
        case "ActorIntentStop": {
            bare.writeU8(bc, 1)
            break
        }
    }
}

/**
 * State
 */
export type ActorStateRunning = null

export type ActorStateStopped = {
    readonly code: StopCode
    readonly message: string | null
}

export function readActorStateStopped(bc: bare.ByteCursor): ActorStateStopped {
    return {
        code: readStopCode(bc),
        message: read17(bc),
    }
}

export function writeActorStateStopped(bc: bare.ByteCursor, x: ActorStateStopped): void {
    writeStopCode(bc, x.code)
    write17(bc, x.message)
}

export type ActorState =
    | { readonly tag: "ActorStateRunning"; readonly val: ActorStateRunning }
    | { readonly tag: "ActorStateStopped"; readonly val: ActorStateStopped }

export function readActorState(bc: bare.ByteCursor): ActorState {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "ActorStateRunning", val: null }
        case 1:
            return { tag: "ActorStateStopped", val: readActorStateStopped(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeActorState(bc: bare.ByteCursor, x: ActorState): void {
    switch (x.tag) {
        case "ActorStateRunning": {
            bare.writeU8(bc, 0)
            break
        }
        case "ActorStateStopped": {
            bare.writeU8(bc, 1)
            writeActorStateStopped(bc, x.val)
            break
        }
    }
}

/**
 * MARK: Events
 */
export type EventActorIntent = {
    readonly intent: ActorIntent
}

export function readEventActorIntent(bc: bare.ByteCursor): EventActorIntent {
    return {
        intent: readActorIntent(bc),
    }
}

export function writeEventActorIntent(bc: bare.ByteCursor, x: EventActorIntent): void {
    writeActorIntent(bc, x.intent)
}

export type EventActorStateUpdate = {
    readonly state: ActorState
}

export function readEventActorStateUpdate(bc: bare.ByteCursor): EventActorStateUpdate {
    return {
        state: readActorState(bc),
    }
}

export function writeEventActorStateUpdate(bc: bare.ByteCursor, x: EventActorStateUpdate): void {
    writeActorState(bc, x.state)
}

export type EventActorSetAlarm = {
    readonly alarmTs: i64 | null
}

export function readEventActorSetAlarm(bc: bare.ByteCursor): EventActorSetAlarm {
    return {
        alarmTs: read12(bc),
    }
}

export function writeEventActorSetAlarm(bc: bare.ByteCursor, x: EventActorSetAlarm): void {
    write12(bc, x.alarmTs)
}

export type Event =
    | { readonly tag: "EventActorIntent"; readonly val: EventActorIntent }
    | { readonly tag: "EventActorStateUpdate"; readonly val: EventActorStateUpdate }
    | { readonly tag: "EventActorSetAlarm"; readonly val: EventActorSetAlarm }

export function readEvent(bc: bare.ByteCursor): Event {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "EventActorIntent", val: readEventActorIntent(bc) }
        case 1:
            return { tag: "EventActorStateUpdate", val: readEventActorStateUpdate(bc) }
        case 2:
            return { tag: "EventActorSetAlarm", val: readEventActorSetAlarm(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeEvent(bc: bare.ByteCursor, x: Event): void {
    switch (x.tag) {
        case "EventActorIntent": {
            bare.writeU8(bc, 0)
            writeEventActorIntent(bc, x.val)
            break
        }
        case "EventActorStateUpdate": {
            bare.writeU8(bc, 1)
            writeEventActorStateUpdate(bc, x.val)
            break
        }
        case "EventActorSetAlarm": {
            bare.writeU8(bc, 2)
            writeEventActorSetAlarm(bc, x.val)
            break
        }
    }
}

export type EventWrapper = {
    readonly checkpoint: ActorCheckpoint
    readonly inner: Event
}

export function readEventWrapper(bc: bare.ByteCursor): EventWrapper {
    return {
        checkpoint: readActorCheckpoint(bc),
        inner: readEvent(bc),
    }
}

export function writeEventWrapper(bc: bare.ByteCursor, x: EventWrapper): void {
    writeActorCheckpoint(bc, x.checkpoint)
    writeEvent(bc, x.inner)
}

export type PreloadedKvEntry = {
    readonly key: KvKey
    readonly value: KvValue
    readonly metadata: KvMetadata
}

export function readPreloadedKvEntry(bc: bare.ByteCursor): PreloadedKvEntry {
    return {
        key: readKvKey(bc),
        value: readKvValue(bc),
        metadata: readKvMetadata(bc),
    }
}

export function writePreloadedKvEntry(bc: bare.ByteCursor, x: PreloadedKvEntry): void {
    writeKvKey(bc, x.key)
    writeKvValue(bc, x.value)
    writeKvMetadata(bc, x.metadata)
}

function read19(bc: bare.ByteCursor): readonly PreloadedKvEntry[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readPreloadedKvEntry(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readPreloadedKvEntry(bc)
    }
    return result
}

function write19(bc: bare.ByteCursor, x: readonly PreloadedKvEntry[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writePreloadedKvEntry(bc, x[i])
    }
}

export type PreloadedKv = {
    readonly entries: readonly PreloadedKvEntry[]
    readonly requestedGetKeys: readonly KvKey[]
    readonly requestedPrefixes: readonly KvKey[]
}

export function readPreloadedKv(bc: bare.ByteCursor): PreloadedKv {
    return {
        entries: read19(bc),
        requestedGetKeys: read0(bc),
        requestedPrefixes: read0(bc),
    }
}

export function writePreloadedKv(bc: bare.ByteCursor, x: PreloadedKv): void {
    write19(bc, x.entries)
    write0(bc, x.requestedGetKeys)
    write0(bc, x.requestedPrefixes)
}

export type HibernatingRequest = {
    readonly gatewayId: GatewayId
    readonly requestId: RequestId
}

export function readHibernatingRequest(bc: bare.ByteCursor): HibernatingRequest {
    return {
        gatewayId: readGatewayId(bc),
        requestId: readRequestId(bc),
    }
}

export function writeHibernatingRequest(bc: bare.ByteCursor, x: HibernatingRequest): void {
    writeGatewayId(bc, x.gatewayId)
    writeRequestId(bc, x.requestId)
}

function read20(bc: bare.ByteCursor): readonly HibernatingRequest[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readHibernatingRequest(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readHibernatingRequest(bc)
    }
    return result
}

function write20(bc: bare.ByteCursor, x: readonly HibernatingRequest[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeHibernatingRequest(bc, x[i])
    }
}

function read21(bc: bare.ByteCursor): PreloadedKv | null {
    return bare.readBool(bc) ? readPreloadedKv(bc) : null
}

function write21(bc: bare.ByteCursor, x: PreloadedKv | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writePreloadedKv(bc, x)
    }
}

export type CommandStartActor = {
    readonly config: ActorConfig
    readonly hibernatingRequests: readonly HibernatingRequest[]
    readonly preloadedKv: PreloadedKv | null
}

export function readCommandStartActor(bc: bare.ByteCursor): CommandStartActor {
    return {
        config: readActorConfig(bc),
        hibernatingRequests: read20(bc),
        preloadedKv: read21(bc),
    }
}

export function writeCommandStartActor(bc: bare.ByteCursor, x: CommandStartActor): void {
    writeActorConfig(bc, x.config)
    write20(bc, x.hibernatingRequests)
    write21(bc, x.preloadedKv)
}

export enum StopActorReason {
    SleepIntent = "SleepIntent",
    StopIntent = "StopIntent",
    Destroy = "Destroy",
    GoingAway = "GoingAway",
    Lost = "Lost",
}

export function readStopActorReason(bc: bare.ByteCursor): StopActorReason {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return StopActorReason.SleepIntent
        case 1:
            return StopActorReason.StopIntent
        case 2:
            return StopActorReason.Destroy
        case 3:
            return StopActorReason.GoingAway
        case 4:
            return StopActorReason.Lost
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeStopActorReason(bc: bare.ByteCursor, x: StopActorReason): void {
    switch (x) {
        case StopActorReason.SleepIntent: {
            bare.writeU8(bc, 0)
            break
        }
        case StopActorReason.StopIntent: {
            bare.writeU8(bc, 1)
            break
        }
        case StopActorReason.Destroy: {
            bare.writeU8(bc, 2)
            break
        }
        case StopActorReason.GoingAway: {
            bare.writeU8(bc, 3)
            break
        }
        case StopActorReason.Lost: {
            bare.writeU8(bc, 4)
            break
        }
    }
}

export type CommandStopActor = {
    readonly reason: StopActorReason
}

export function readCommandStopActor(bc: bare.ByteCursor): CommandStopActor {
    return {
        reason: readStopActorReason(bc),
    }
}

export function writeCommandStopActor(bc: bare.ByteCursor, x: CommandStopActor): void {
    writeStopActorReason(bc, x.reason)
}

export type Command =
    | { readonly tag: "CommandStartActor"; readonly val: CommandStartActor }
    | { readonly tag: "CommandStopActor"; readonly val: CommandStopActor }

export function readCommand(bc: bare.ByteCursor): Command {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "CommandStartActor", val: readCommandStartActor(bc) }
        case 1:
            return { tag: "CommandStopActor", val: readCommandStopActor(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeCommand(bc: bare.ByteCursor, x: Command): void {
    switch (x.tag) {
        case "CommandStartActor": {
            bare.writeU8(bc, 0)
            writeCommandStartActor(bc, x.val)
            break
        }
        case "CommandStopActor": {
            bare.writeU8(bc, 1)
            writeCommandStopActor(bc, x.val)
            break
        }
    }
}

export type CommandWrapper = {
    readonly checkpoint: ActorCheckpoint
    readonly inner: Command
}

export function readCommandWrapper(bc: bare.ByteCursor): CommandWrapper {
    return {
        checkpoint: readActorCheckpoint(bc),
        inner: readCommand(bc),
    }
}

export function writeCommandWrapper(bc: bare.ByteCursor, x: CommandWrapper): void {
    writeActorCheckpoint(bc, x.checkpoint)
    writeCommand(bc, x.inner)
}

/**
 * We redeclare this so its top level
 */
export type ActorCommandKeyData =
    | { readonly tag: "CommandStartActor"; readonly val: CommandStartActor }
    | { readonly tag: "CommandStopActor"; readonly val: CommandStopActor }

export function readActorCommandKeyData(bc: bare.ByteCursor): ActorCommandKeyData {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "CommandStartActor", val: readCommandStartActor(bc) }
        case 1:
            return { tag: "CommandStopActor", val: readCommandStopActor(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeActorCommandKeyData(bc: bare.ByteCursor, x: ActorCommandKeyData): void {
    switch (x.tag) {
        case "CommandStartActor": {
            bare.writeU8(bc, 0)
            writeCommandStartActor(bc, x.val)
            break
        }
        case "CommandStopActor": {
            bare.writeU8(bc, 1)
            writeCommandStopActor(bc, x.val)
            break
        }
    }
}

export function encodeActorCommandKeyData(x: ActorCommandKeyData, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeActorCommandKeyData(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeActorCommandKeyData(bytes: Uint8Array): ActorCommandKeyData {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readActorCommandKeyData(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}

export type MessageId = {
    /**
     * Globally unique ID
     */
    readonly gatewayId: GatewayId
    /**
     * Unique ID to the gateway
     */
    readonly requestId: RequestId
    /**
     * Unique ID to the request
     */
    readonly messageIndex: MessageIndex
}

export function readMessageId(bc: bare.ByteCursor): MessageId {
    return {
        gatewayId: readGatewayId(bc),
        requestId: readRequestId(bc),
        messageIndex: readMessageIndex(bc),
    }
}

export function writeMessageId(bc: bare.ByteCursor, x: MessageId): void {
    writeGatewayId(bc, x.gatewayId)
    writeRequestId(bc, x.requestId)
    writeMessageIndex(bc, x.messageIndex)
}

function read22(bc: bare.ByteCursor): ReadonlyMap<string, string> {
    const len = bare.readUintSafe(bc)
    const result = new Map<string, string>()
    for (let i = 0; i < len; i++) {
        const offset = bc.offset
        const key = bare.readString(bc)
        if (result.has(key)) {
            bc.offset = offset
            throw new bare.BareError(offset, "duplicated key")
        }
        result.set(key, bare.readString(bc))
    }
    return result
}

function write22(bc: bare.ByteCursor, x: ReadonlyMap<string, string>): void {
    bare.writeUintSafe(bc, x.size)
    for (const kv of x) {
        bare.writeString(bc, kv[0])
        bare.writeString(bc, kv[1])
    }
}

/**
 * HTTP
 */
export type ToEnvoyRequestStart = {
    readonly actorId: Id
    readonly method: string
    readonly path: string
    readonly headers: ReadonlyMap<string, string>
    readonly body: ArrayBuffer | null
    readonly stream: boolean
}

export function readToEnvoyRequestStart(bc: bare.ByteCursor): ToEnvoyRequestStart {
    return {
        actorId: readId(bc),
        method: bare.readString(bc),
        path: bare.readString(bc),
        headers: read22(bc),
        body: read18(bc),
        stream: bare.readBool(bc),
    }
}

export function writeToEnvoyRequestStart(bc: bare.ByteCursor, x: ToEnvoyRequestStart): void {
    writeId(bc, x.actorId)
    bare.writeString(bc, x.method)
    bare.writeString(bc, x.path)
    write22(bc, x.headers)
    write18(bc, x.body)
    bare.writeBool(bc, x.stream)
}

export type ToEnvoyRequestChunk = {
    readonly body: ArrayBuffer
    readonly finish: boolean
}

export function readToEnvoyRequestChunk(bc: bare.ByteCursor): ToEnvoyRequestChunk {
    return {
        body: bare.readData(bc),
        finish: bare.readBool(bc),
    }
}

export function writeToEnvoyRequestChunk(bc: bare.ByteCursor, x: ToEnvoyRequestChunk): void {
    bare.writeData(bc, x.body)
    bare.writeBool(bc, x.finish)
}

export enum HttpStreamAbortReasonKind {
    Unknown = "Unknown",
    ClientDisconnect = "ClientDisconnect",
    HandlerError = "HandlerError",
    IdleTimeout = "IdleTimeout",
    Overloaded = "Overloaded",
    BodyTooLarge = "BodyTooLarge",
    OutOfMemory = "OutOfMemory",
    Shutdown = "Shutdown",
    InternalError = "InternalError",
}

export function readHttpStreamAbortReasonKind(bc: bare.ByteCursor): HttpStreamAbortReasonKind {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return HttpStreamAbortReasonKind.Unknown
        case 1:
            return HttpStreamAbortReasonKind.ClientDisconnect
        case 2:
            return HttpStreamAbortReasonKind.HandlerError
        case 3:
            return HttpStreamAbortReasonKind.IdleTimeout
        case 4:
            return HttpStreamAbortReasonKind.Overloaded
        case 5:
            return HttpStreamAbortReasonKind.BodyTooLarge
        case 6:
            return HttpStreamAbortReasonKind.OutOfMemory
        case 7:
            return HttpStreamAbortReasonKind.Shutdown
        case 8:
            return HttpStreamAbortReasonKind.InternalError
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeHttpStreamAbortReasonKind(bc: bare.ByteCursor, x: HttpStreamAbortReasonKind): void {
    switch (x) {
        case HttpStreamAbortReasonKind.Unknown: {
            bare.writeU8(bc, 0)
            break
        }
        case HttpStreamAbortReasonKind.ClientDisconnect: {
            bare.writeU8(bc, 1)
            break
        }
        case HttpStreamAbortReasonKind.HandlerError: {
            bare.writeU8(bc, 2)
            break
        }
        case HttpStreamAbortReasonKind.IdleTimeout: {
            bare.writeU8(bc, 3)
            break
        }
        case HttpStreamAbortReasonKind.Overloaded: {
            bare.writeU8(bc, 4)
            break
        }
        case HttpStreamAbortReasonKind.BodyTooLarge: {
            bare.writeU8(bc, 5)
            break
        }
        case HttpStreamAbortReasonKind.OutOfMemory: {
            bare.writeU8(bc, 6)
            break
        }
        case HttpStreamAbortReasonKind.Shutdown: {
            bare.writeU8(bc, 7)
            break
        }
        case HttpStreamAbortReasonKind.InternalError: {
            bare.writeU8(bc, 8)
            break
        }
    }
}

export type HttpStreamAbortReason = {
    readonly kind: HttpStreamAbortReasonKind
    readonly detail: string | null
}

export function readHttpStreamAbortReason(bc: bare.ByteCursor): HttpStreamAbortReason {
    return {
        kind: readHttpStreamAbortReasonKind(bc),
        detail: read17(bc),
    }
}

export function writeHttpStreamAbortReason(bc: bare.ByteCursor, x: HttpStreamAbortReason): void {
    writeHttpStreamAbortReasonKind(bc, x.kind)
    write17(bc, x.detail)
}

export type ToEnvoyRequestAbort = {
    readonly reason: HttpStreamAbortReason
}

export function readToEnvoyRequestAbort(bc: bare.ByteCursor): ToEnvoyRequestAbort {
    return {
        reason: readHttpStreamAbortReason(bc),
    }
}

export function writeToEnvoyRequestAbort(bc: bare.ByteCursor, x: ToEnvoyRequestAbort): void {
    writeHttpStreamAbortReason(bc, x.reason)
}

export type ToRivetResponseStart = {
    readonly status: u16
    readonly headers: ReadonlyMap<string, string>
    readonly body: ArrayBuffer | null
    readonly stream: boolean
}

export function readToRivetResponseStart(bc: bare.ByteCursor): ToRivetResponseStart {
    return {
        status: bare.readU16(bc),
        headers: read22(bc),
        body: read18(bc),
        stream: bare.readBool(bc),
    }
}

export function writeToRivetResponseStart(bc: bare.ByteCursor, x: ToRivetResponseStart): void {
    bare.writeU16(bc, x.status)
    write22(bc, x.headers)
    write18(bc, x.body)
    bare.writeBool(bc, x.stream)
}

export type ToRivetResponseChunk = {
    readonly body: ArrayBuffer
    readonly finish: boolean
}

export function readToRivetResponseChunk(bc: bare.ByteCursor): ToRivetResponseChunk {
    return {
        body: bare.readData(bc),
        finish: bare.readBool(bc),
    }
}

export function writeToRivetResponseChunk(bc: bare.ByteCursor, x: ToRivetResponseChunk): void {
    bare.writeData(bc, x.body)
    bare.writeBool(bc, x.finish)
}

export type ToRivetResponseAbort = {
    readonly reason: HttpStreamAbortReason
}

export function readToRivetResponseAbort(bc: bare.ByteCursor): ToRivetResponseAbort {
    return {
        reason: readHttpStreamAbortReason(bc),
    }
}

export function writeToRivetResponseAbort(bc: bare.ByteCursor, x: ToRivetResponseAbort): void {
    writeHttpStreamAbortReason(bc, x.reason)
}

/**
 * WebSocket
 */
export type ToEnvoyWebSocketOpen = {
    readonly actorId: Id
    readonly path: string
    readonly headers: ReadonlyMap<string, string>
}

export function readToEnvoyWebSocketOpen(bc: bare.ByteCursor): ToEnvoyWebSocketOpen {
    return {
        actorId: readId(bc),
        path: bare.readString(bc),
        headers: read22(bc),
    }
}

export function writeToEnvoyWebSocketOpen(bc: bare.ByteCursor, x: ToEnvoyWebSocketOpen): void {
    writeId(bc, x.actorId)
    bare.writeString(bc, x.path)
    write22(bc, x.headers)
}

export type ToEnvoyWebSocketMessage = {
    readonly data: ArrayBuffer
    readonly binary: boolean
}

export function readToEnvoyWebSocketMessage(bc: bare.ByteCursor): ToEnvoyWebSocketMessage {
    return {
        data: bare.readData(bc),
        binary: bare.readBool(bc),
    }
}

export function writeToEnvoyWebSocketMessage(bc: bare.ByteCursor, x: ToEnvoyWebSocketMessage): void {
    bare.writeData(bc, x.data)
    bare.writeBool(bc, x.binary)
}

function read23(bc: bare.ByteCursor): u16 | null {
    return bare.readBool(bc) ? bare.readU16(bc) : null
}

function write23(bc: bare.ByteCursor, x: u16 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeU16(bc, x)
    }
}

export type ToEnvoyWebSocketClose = {
    readonly code: u16 | null
    readonly reason: string | null
}

export function readToEnvoyWebSocketClose(bc: bare.ByteCursor): ToEnvoyWebSocketClose {
    return {
        code: read23(bc),
        reason: read17(bc),
    }
}

export function writeToEnvoyWebSocketClose(bc: bare.ByteCursor, x: ToEnvoyWebSocketClose): void {
    write23(bc, x.code)
    write17(bc, x.reason)
}

export type ToRivetWebSocketOpen = {
    readonly canHibernate: boolean
}

export function readToRivetWebSocketOpen(bc: bare.ByteCursor): ToRivetWebSocketOpen {
    return {
        canHibernate: bare.readBool(bc),
    }
}

export function writeToRivetWebSocketOpen(bc: bare.ByteCursor, x: ToRivetWebSocketOpen): void {
    bare.writeBool(bc, x.canHibernate)
}

export type ToRivetWebSocketMessage = {
    readonly data: ArrayBuffer
    readonly binary: boolean
}

export function readToRivetWebSocketMessage(bc: bare.ByteCursor): ToRivetWebSocketMessage {
    return {
        data: bare.readData(bc),
        binary: bare.readBool(bc),
    }
}

export function writeToRivetWebSocketMessage(bc: bare.ByteCursor, x: ToRivetWebSocketMessage): void {
    bare.writeData(bc, x.data)
    bare.writeBool(bc, x.binary)
}

export type ToRivetWebSocketMessageAck = {
    readonly index: MessageIndex
}

export function readToRivetWebSocketMessageAck(bc: bare.ByteCursor): ToRivetWebSocketMessageAck {
    return {
        index: readMessageIndex(bc),
    }
}

export function writeToRivetWebSocketMessageAck(bc: bare.ByteCursor, x: ToRivetWebSocketMessageAck): void {
    writeMessageIndex(bc, x.index)
}

export type ToRivetWebSocketClose = {
    readonly code: u16 | null
    readonly reason: string | null
    readonly hibernate: boolean
}

export function readToRivetWebSocketClose(bc: bare.ByteCursor): ToRivetWebSocketClose {
    return {
        code: read23(bc),
        reason: read17(bc),
        hibernate: bare.readBool(bc),
    }
}

export function writeToRivetWebSocketClose(bc: bare.ByteCursor, x: ToRivetWebSocketClose): void {
    write23(bc, x.code)
    write17(bc, x.reason)
    bare.writeBool(bc, x.hibernate)
}

/**
 * To Rivet
 */
export type ToRivetTunnelMessageKind =
    /**
     * HTTP
     */
    | { readonly tag: "ToRivetResponseStart"; readonly val: ToRivetResponseStart }
    | { readonly tag: "ToRivetResponseChunk"; readonly val: ToRivetResponseChunk }
    | { readonly tag: "ToRivetResponseAbort"; readonly val: ToRivetResponseAbort }
    /**
     * WebSocket
     */
    | { readonly tag: "ToRivetWebSocketOpen"; readonly val: ToRivetWebSocketOpen }
    | { readonly tag: "ToRivetWebSocketMessage"; readonly val: ToRivetWebSocketMessage }
    | { readonly tag: "ToRivetWebSocketMessageAck"; readonly val: ToRivetWebSocketMessageAck }
    | { readonly tag: "ToRivetWebSocketClose"; readonly val: ToRivetWebSocketClose }

export function readToRivetTunnelMessageKind(bc: bare.ByteCursor): ToRivetTunnelMessageKind {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "ToRivetResponseStart", val: readToRivetResponseStart(bc) }
        case 1:
            return { tag: "ToRivetResponseChunk", val: readToRivetResponseChunk(bc) }
        case 2:
            return { tag: "ToRivetResponseAbort", val: readToRivetResponseAbort(bc) }
        case 3:
            return { tag: "ToRivetWebSocketOpen", val: readToRivetWebSocketOpen(bc) }
        case 4:
            return { tag: "ToRivetWebSocketMessage", val: readToRivetWebSocketMessage(bc) }
        case 5:
            return { tag: "ToRivetWebSocketMessageAck", val: readToRivetWebSocketMessageAck(bc) }
        case 6:
            return { tag: "ToRivetWebSocketClose", val: readToRivetWebSocketClose(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeToRivetTunnelMessageKind(bc: bare.ByteCursor, x: ToRivetTunnelMessageKind): void {
    switch (x.tag) {
        case "ToRivetResponseStart": {
            bare.writeU8(bc, 0)
            writeToRivetResponseStart(bc, x.val)
            break
        }
        case "ToRivetResponseChunk": {
            bare.writeU8(bc, 1)
            writeToRivetResponseChunk(bc, x.val)
            break
        }
        case "ToRivetResponseAbort": {
            bare.writeU8(bc, 2)
            writeToRivetResponseAbort(bc, x.val)
            break
        }
        case "ToRivetWebSocketOpen": {
            bare.writeU8(bc, 3)
            writeToRivetWebSocketOpen(bc, x.val)
            break
        }
        case "ToRivetWebSocketMessage": {
            bare.writeU8(bc, 4)
            writeToRivetWebSocketMessage(bc, x.val)
            break
        }
        case "ToRivetWebSocketMessageAck": {
            bare.writeU8(bc, 5)
            writeToRivetWebSocketMessageAck(bc, x.val)
            break
        }
        case "ToRivetWebSocketClose": {
            bare.writeU8(bc, 6)
            writeToRivetWebSocketClose(bc, x.val)
            break
        }
    }
}

export type ToRivetTunnelMessage = {
    readonly messageId: MessageId
    readonly messageKind: ToRivetTunnelMessageKind
}

export function readToRivetTunnelMessage(bc: bare.ByteCursor): ToRivetTunnelMessage {
    return {
        messageId: readMessageId(bc),
        messageKind: readToRivetTunnelMessageKind(bc),
    }
}

export function writeToRivetTunnelMessage(bc: bare.ByteCursor, x: ToRivetTunnelMessage): void {
    writeMessageId(bc, x.messageId)
    writeToRivetTunnelMessageKind(bc, x.messageKind)
}

/**
 * To Envoy
 */
export type ToEnvoyTunnelMessageKind =
    /**
     * HTTP
     */
    | { readonly tag: "ToEnvoyRequestStart"; readonly val: ToEnvoyRequestStart }
    | { readonly tag: "ToEnvoyRequestChunk"; readonly val: ToEnvoyRequestChunk }
    | { readonly tag: "ToEnvoyRequestAbort"; readonly val: ToEnvoyRequestAbort }
    /**
     * WebSocket
     */
    | { readonly tag: "ToEnvoyWebSocketOpen"; readonly val: ToEnvoyWebSocketOpen }
    | { readonly tag: "ToEnvoyWebSocketMessage"; readonly val: ToEnvoyWebSocketMessage }
    | { readonly tag: "ToEnvoyWebSocketClose"; readonly val: ToEnvoyWebSocketClose }

export function readToEnvoyTunnelMessageKind(bc: bare.ByteCursor): ToEnvoyTunnelMessageKind {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "ToEnvoyRequestStart", val: readToEnvoyRequestStart(bc) }
        case 1:
            return { tag: "ToEnvoyRequestChunk", val: readToEnvoyRequestChunk(bc) }
        case 2:
            return { tag: "ToEnvoyRequestAbort", val: readToEnvoyRequestAbort(bc) }
        case 3:
            return { tag: "ToEnvoyWebSocketOpen", val: readToEnvoyWebSocketOpen(bc) }
        case 4:
            return { tag: "ToEnvoyWebSocketMessage", val: readToEnvoyWebSocketMessage(bc) }
        case 5:
            return { tag: "ToEnvoyWebSocketClose", val: readToEnvoyWebSocketClose(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeToEnvoyTunnelMessageKind(bc: bare.ByteCursor, x: ToEnvoyTunnelMessageKind): void {
    switch (x.tag) {
        case "ToEnvoyRequestStart": {
            bare.writeU8(bc, 0)
            writeToEnvoyRequestStart(bc, x.val)
            break
        }
        case "ToEnvoyRequestChunk": {
            bare.writeU8(bc, 1)
            writeToEnvoyRequestChunk(bc, x.val)
            break
        }
        case "ToEnvoyRequestAbort": {
            bare.writeU8(bc, 2)
            writeToEnvoyRequestAbort(bc, x.val)
            break
        }
        case "ToEnvoyWebSocketOpen": {
            bare.writeU8(bc, 3)
            writeToEnvoyWebSocketOpen(bc, x.val)
            break
        }
        case "ToEnvoyWebSocketMessage": {
            bare.writeU8(bc, 4)
            writeToEnvoyWebSocketMessage(bc, x.val)
            break
        }
        case "ToEnvoyWebSocketClose": {
            bare.writeU8(bc, 5)
            writeToEnvoyWebSocketClose(bc, x.val)
            break
        }
    }
}

export type ToEnvoyTunnelMessage = {
    readonly messageId: MessageId
    readonly messageKind: ToEnvoyTunnelMessageKind
}

export function readToEnvoyTunnelMessage(bc: bare.ByteCursor): ToEnvoyTunnelMessage {
    return {
        messageId: readMessageId(bc),
        messageKind: readToEnvoyTunnelMessageKind(bc),
    }
}

export function writeToEnvoyTunnelMessage(bc: bare.ByteCursor, x: ToEnvoyTunnelMessage): void {
    writeMessageId(bc, x.messageId)
    writeToEnvoyTunnelMessageKind(bc, x.messageKind)
}

export type ToEnvoyPing = {
    readonly ts: i64
}

export function readToEnvoyPing(bc: bare.ByteCursor): ToEnvoyPing {
    return {
        ts: bare.readI64(bc),
    }
}

export function writeToEnvoyPing(bc: bare.ByteCursor, x: ToEnvoyPing): void {
    bare.writeI64(bc, x.ts)
}

function read24(bc: bare.ByteCursor): ReadonlyMap<string, ActorName> {
    const len = bare.readUintSafe(bc)
    const result = new Map<string, ActorName>()
    for (let i = 0; i < len; i++) {
        const offset = bc.offset
        const key = bare.readString(bc)
        if (result.has(key)) {
            bc.offset = offset
            throw new bare.BareError(offset, "duplicated key")
        }
        result.set(key, readActorName(bc))
    }
    return result
}

function write24(bc: bare.ByteCursor, x: ReadonlyMap<string, ActorName>): void {
    bare.writeUintSafe(bc, x.size)
    for (const kv of x) {
        bare.writeString(bc, kv[0])
        writeActorName(bc, kv[1])
    }
}

function read25(bc: bare.ByteCursor): ReadonlyMap<string, ActorName> | null {
    return bare.readBool(bc) ? read24(bc) : null
}

function write25(bc: bare.ByteCursor, x: ReadonlyMap<string, ActorName> | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        write24(bc, x)
    }
}

function read26(bc: bare.ByteCursor): Json | null {
    return bare.readBool(bc) ? readJson(bc) : null
}

function write26(bc: bare.ByteCursor, x: Json | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeJson(bc, x)
    }
}

/**
 * MARK: To Rivet
 */
export type ToRivetMetadata = {
    readonly prepopulateActorNames: ReadonlyMap<string, ActorName> | null
    readonly metadata: Json | null
}

export function readToRivetMetadata(bc: bare.ByteCursor): ToRivetMetadata {
    return {
        prepopulateActorNames: read25(bc),
        metadata: read26(bc),
    }
}

export function writeToRivetMetadata(bc: bare.ByteCursor, x: ToRivetMetadata): void {
    write25(bc, x.prepopulateActorNames)
    write26(bc, x.metadata)
}

export type ToRivetEvents = readonly EventWrapper[]

export function readToRivetEvents(bc: bare.ByteCursor): ToRivetEvents {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readEventWrapper(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readEventWrapper(bc)
    }
    return result
}

export function writeToRivetEvents(bc: bare.ByteCursor, x: ToRivetEvents): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeEventWrapper(bc, x[i])
    }
}

function read27(bc: bare.ByteCursor): readonly ActorCheckpoint[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readActorCheckpoint(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readActorCheckpoint(bc)
    }
    return result
}

function write27(bc: bare.ByteCursor, x: readonly ActorCheckpoint[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeActorCheckpoint(bc, x[i])
    }
}

export type ToRivetAckCommands = {
    readonly lastCommandCheckpoints: readonly ActorCheckpoint[]
}

export function readToRivetAckCommands(bc: bare.ByteCursor): ToRivetAckCommands {
    return {
        lastCommandCheckpoints: read27(bc),
    }
}

export function writeToRivetAckCommands(bc: bare.ByteCursor, x: ToRivetAckCommands): void {
    write27(bc, x.lastCommandCheckpoints)
}

export type ToRivetStopping = null

export type ToRivetPong = {
    readonly ts: i64
}

export function readToRivetPong(bc: bare.ByteCursor): ToRivetPong {
    return {
        ts: bare.readI64(bc),
    }
}

export function writeToRivetPong(bc: bare.ByteCursor, x: ToRivetPong): void {
    bare.writeI64(bc, x.ts)
}

export type ToRivetKvRequest = {
    readonly actorId: Id
    readonly requestId: u32
    readonly data: KvRequestData
}

export function readToRivetKvRequest(bc: bare.ByteCursor): ToRivetKvRequest {
    return {
        actorId: readId(bc),
        requestId: bare.readU32(bc),
        data: readKvRequestData(bc),
    }
}

export function writeToRivetKvRequest(bc: bare.ByteCursor, x: ToRivetKvRequest): void {
    writeId(bc, x.actorId)
    bare.writeU32(bc, x.requestId)
    writeKvRequestData(bc, x.data)
}

export type ToRivetSqliteGetPagesRequest = {
    readonly requestId: u32
    readonly data: SqliteGetPagesRequest
}

export function readToRivetSqliteGetPagesRequest(bc: bare.ByteCursor): ToRivetSqliteGetPagesRequest {
    return {
        requestId: bare.readU32(bc),
        data: readSqliteGetPagesRequest(bc),
    }
}

export function writeToRivetSqliteGetPagesRequest(bc: bare.ByteCursor, x: ToRivetSqliteGetPagesRequest): void {
    bare.writeU32(bc, x.requestId)
    writeSqliteGetPagesRequest(bc, x.data)
}

export type ToRivetSqliteCommitRequest = {
    readonly requestId: u32
    readonly data: SqliteCommitRequest
}

export function readToRivetSqliteCommitRequest(bc: bare.ByteCursor): ToRivetSqliteCommitRequest {
    return {
        requestId: bare.readU32(bc),
        data: readSqliteCommitRequest(bc),
    }
}

export function writeToRivetSqliteCommitRequest(bc: bare.ByteCursor, x: ToRivetSqliteCommitRequest): void {
    bare.writeU32(bc, x.requestId)
    writeSqliteCommitRequest(bc, x.data)
}

export type ToRivetSqliteExecRequest = {
    readonly requestId: u32
    readonly data: SqliteExecRequest
}

export function readToRivetSqliteExecRequest(bc: bare.ByteCursor): ToRivetSqliteExecRequest {
    return {
        requestId: bare.readU32(bc),
        data: readSqliteExecRequest(bc),
    }
}

export function writeToRivetSqliteExecRequest(bc: bare.ByteCursor, x: ToRivetSqliteExecRequest): void {
    bare.writeU32(bc, x.requestId)
    writeSqliteExecRequest(bc, x.data)
}

export type ToRivetSqliteExecuteRequest = {
    readonly requestId: u32
    readonly data: SqliteExecuteRequest
}

export function readToRivetSqliteExecuteRequest(bc: bare.ByteCursor): ToRivetSqliteExecuteRequest {
    return {
        requestId: bare.readU32(bc),
        data: readSqliteExecuteRequest(bc),
    }
}

export function writeToRivetSqliteExecuteRequest(bc: bare.ByteCursor, x: ToRivetSqliteExecuteRequest): void {
    bare.writeU32(bc, x.requestId)
    writeSqliteExecuteRequest(bc, x.data)
}

export type ToRivetSqliteExecuteBatchRequest = {
    readonly requestId: u32
    readonly data: SqliteExecuteBatchRequest
}

export function readToRivetSqliteExecuteBatchRequest(bc: bare.ByteCursor): ToRivetSqliteExecuteBatchRequest {
    return {
        requestId: bare.readU32(bc),
        data: readSqliteExecuteBatchRequest(bc),
    }
}

export function writeToRivetSqliteExecuteBatchRequest(bc: bare.ByteCursor, x: ToRivetSqliteExecuteBatchRequest): void {
    bare.writeU32(bc, x.requestId)
    writeSqliteExecuteBatchRequest(bc, x.data)
}

export type ToRivet =
    | { readonly tag: "ToRivetMetadata"; readonly val: ToRivetMetadata }
    | { readonly tag: "ToRivetEvents"; readonly val: ToRivetEvents }
    | { readonly tag: "ToRivetAckCommands"; readonly val: ToRivetAckCommands }
    | { readonly tag: "ToRivetStopping"; readonly val: ToRivetStopping }
    | { readonly tag: "ToRivetPong"; readonly val: ToRivetPong }
    | { readonly tag: "ToRivetKvRequest"; readonly val: ToRivetKvRequest }
    | { readonly tag: "ToRivetTunnelMessage"; readonly val: ToRivetTunnelMessage }
    | { readonly tag: "ToRivetSqliteGetPagesRequest"; readonly val: ToRivetSqliteGetPagesRequest }
    | { readonly tag: "ToRivetSqliteCommitRequest"; readonly val: ToRivetSqliteCommitRequest }
    | { readonly tag: "ToRivetSqliteExecRequest"; readonly val: ToRivetSqliteExecRequest }
    | { readonly tag: "ToRivetSqliteExecuteRequest"; readonly val: ToRivetSqliteExecuteRequest }
    | { readonly tag: "ToRivetSqliteExecuteBatchRequest"; readonly val: ToRivetSqliteExecuteBatchRequest }

export function readToRivet(bc: bare.ByteCursor): ToRivet {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "ToRivetMetadata", val: readToRivetMetadata(bc) }
        case 1:
            return { tag: "ToRivetEvents", val: readToRivetEvents(bc) }
        case 2:
            return { tag: "ToRivetAckCommands", val: readToRivetAckCommands(bc) }
        case 3:
            return { tag: "ToRivetStopping", val: null }
        case 4:
            return { tag: "ToRivetPong", val: readToRivetPong(bc) }
        case 5:
            return { tag: "ToRivetKvRequest", val: readToRivetKvRequest(bc) }
        case 6:
            return { tag: "ToRivetTunnelMessage", val: readToRivetTunnelMessage(bc) }
        case 7:
            return { tag: "ToRivetSqliteGetPagesRequest", val: readToRivetSqliteGetPagesRequest(bc) }
        case 8:
            return { tag: "ToRivetSqliteCommitRequest", val: readToRivetSqliteCommitRequest(bc) }
        case 9:
            return { tag: "ToRivetSqliteExecRequest", val: readToRivetSqliteExecRequest(bc) }
        case 10:
            return { tag: "ToRivetSqliteExecuteRequest", val: readToRivetSqliteExecuteRequest(bc) }
        case 11:
            return { tag: "ToRivetSqliteExecuteBatchRequest", val: readToRivetSqliteExecuteBatchRequest(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeToRivet(bc: bare.ByteCursor, x: ToRivet): void {
    switch (x.tag) {
        case "ToRivetMetadata": {
            bare.writeU8(bc, 0)
            writeToRivetMetadata(bc, x.val)
            break
        }
        case "ToRivetEvents": {
            bare.writeU8(bc, 1)
            writeToRivetEvents(bc, x.val)
            break
        }
        case "ToRivetAckCommands": {
            bare.writeU8(bc, 2)
            writeToRivetAckCommands(bc, x.val)
            break
        }
        case "ToRivetStopping": {
            bare.writeU8(bc, 3)
            break
        }
        case "ToRivetPong": {
            bare.writeU8(bc, 4)
            writeToRivetPong(bc, x.val)
            break
        }
        case "ToRivetKvRequest": {
            bare.writeU8(bc, 5)
            writeToRivetKvRequest(bc, x.val)
            break
        }
        case "ToRivetTunnelMessage": {
            bare.writeU8(bc, 6)
            writeToRivetTunnelMessage(bc, x.val)
            break
        }
        case "ToRivetSqliteGetPagesRequest": {
            bare.writeU8(bc, 7)
            writeToRivetSqliteGetPagesRequest(bc, x.val)
            break
        }
        case "ToRivetSqliteCommitRequest": {
            bare.writeU8(bc, 8)
            writeToRivetSqliteCommitRequest(bc, x.val)
            break
        }
        case "ToRivetSqliteExecRequest": {
            bare.writeU8(bc, 9)
            writeToRivetSqliteExecRequest(bc, x.val)
            break
        }
        case "ToRivetSqliteExecuteRequest": {
            bare.writeU8(bc, 10)
            writeToRivetSqliteExecuteRequest(bc, x.val)
            break
        }
        case "ToRivetSqliteExecuteBatchRequest": {
            bare.writeU8(bc, 11)
            writeToRivetSqliteExecuteBatchRequest(bc, x.val)
            break
        }
    }
}

export function encodeToRivet(x: ToRivet, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeToRivet(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeToRivet(bytes: Uint8Array): ToRivet {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readToRivet(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}

/**
 * MARK: To Envoy
 */
export type ProtocolMetadata = {
    readonly envoyLostThreshold: i64
    readonly actorStopThreshold: i64
    readonly maxResponsePayloadSize: u64
}

export function readProtocolMetadata(bc: bare.ByteCursor): ProtocolMetadata {
    return {
        envoyLostThreshold: bare.readI64(bc),
        actorStopThreshold: bare.readI64(bc),
        maxResponsePayloadSize: bare.readU64(bc),
    }
}

export function writeProtocolMetadata(bc: bare.ByteCursor, x: ProtocolMetadata): void {
    bare.writeI64(bc, x.envoyLostThreshold)
    bare.writeI64(bc, x.actorStopThreshold)
    bare.writeU64(bc, x.maxResponsePayloadSize)
}

export type ToEnvoyInit = {
    readonly metadata: ProtocolMetadata
}

export function readToEnvoyInit(bc: bare.ByteCursor): ToEnvoyInit {
    return {
        metadata: readProtocolMetadata(bc),
    }
}

export function writeToEnvoyInit(bc: bare.ByteCursor, x: ToEnvoyInit): void {
    writeProtocolMetadata(bc, x.metadata)
}

export type ToEnvoyCommands = readonly CommandWrapper[]

export function readToEnvoyCommands(bc: bare.ByteCursor): ToEnvoyCommands {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readCommandWrapper(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readCommandWrapper(bc)
    }
    return result
}

export function writeToEnvoyCommands(bc: bare.ByteCursor, x: ToEnvoyCommands): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeCommandWrapper(bc, x[i])
    }
}

export type ToEnvoyAckEvents = {
    readonly lastEventCheckpoints: readonly ActorCheckpoint[]
}

export function readToEnvoyAckEvents(bc: bare.ByteCursor): ToEnvoyAckEvents {
    return {
        lastEventCheckpoints: read27(bc),
    }
}

export function writeToEnvoyAckEvents(bc: bare.ByteCursor, x: ToEnvoyAckEvents): void {
    write27(bc, x.lastEventCheckpoints)
}

export type ToEnvoyKvResponse = {
    readonly requestId: u32
    readonly data: KvResponseData
}

export function readToEnvoyKvResponse(bc: bare.ByteCursor): ToEnvoyKvResponse {
    return {
        requestId: bare.readU32(bc),
        data: readKvResponseData(bc),
    }
}

export function writeToEnvoyKvResponse(bc: bare.ByteCursor, x: ToEnvoyKvResponse): void {
    bare.writeU32(bc, x.requestId)
    writeKvResponseData(bc, x.data)
}

export type ToEnvoySqliteGetPagesResponse = {
    readonly requestId: u32
    readonly data: SqliteGetPagesResponse
}

export function readToEnvoySqliteGetPagesResponse(bc: bare.ByteCursor): ToEnvoySqliteGetPagesResponse {
    return {
        requestId: bare.readU32(bc),
        data: readSqliteGetPagesResponse(bc),
    }
}

export function writeToEnvoySqliteGetPagesResponse(bc: bare.ByteCursor, x: ToEnvoySqliteGetPagesResponse): void {
    bare.writeU32(bc, x.requestId)
    writeSqliteGetPagesResponse(bc, x.data)
}

export type ToEnvoySqliteCommitResponse = {
    readonly requestId: u32
    readonly data: SqliteCommitResponse
}

export function readToEnvoySqliteCommitResponse(bc: bare.ByteCursor): ToEnvoySqliteCommitResponse {
    return {
        requestId: bare.readU32(bc),
        data: readSqliteCommitResponse(bc),
    }
}

export function writeToEnvoySqliteCommitResponse(bc: bare.ByteCursor, x: ToEnvoySqliteCommitResponse): void {
    bare.writeU32(bc, x.requestId)
    writeSqliteCommitResponse(bc, x.data)
}

export type ToEnvoySqliteExecResponse = {
    readonly requestId: u32
    readonly data: SqliteExecResponse
}

export function readToEnvoySqliteExecResponse(bc: bare.ByteCursor): ToEnvoySqliteExecResponse {
    return {
        requestId: bare.readU32(bc),
        data: readSqliteExecResponse(bc),
    }
}

export function writeToEnvoySqliteExecResponse(bc: bare.ByteCursor, x: ToEnvoySqliteExecResponse): void {
    bare.writeU32(bc, x.requestId)
    writeSqliteExecResponse(bc, x.data)
}

export type ToEnvoySqliteExecuteResponse = {
    readonly requestId: u32
    readonly data: SqliteExecuteResponse
}

export function readToEnvoySqliteExecuteResponse(bc: bare.ByteCursor): ToEnvoySqliteExecuteResponse {
    return {
        requestId: bare.readU32(bc),
        data: readSqliteExecuteResponse(bc),
    }
}

export function writeToEnvoySqliteExecuteResponse(bc: bare.ByteCursor, x: ToEnvoySqliteExecuteResponse): void {
    bare.writeU32(bc, x.requestId)
    writeSqliteExecuteResponse(bc, x.data)
}

export type ToEnvoySqliteExecuteBatchResponse = {
    readonly requestId: u32
    readonly data: SqliteExecuteBatchResponse
}

export function readToEnvoySqliteExecuteBatchResponse(bc: bare.ByteCursor): ToEnvoySqliteExecuteBatchResponse {
    return {
        requestId: bare.readU32(bc),
        data: readSqliteExecuteBatchResponse(bc),
    }
}

export function writeToEnvoySqliteExecuteBatchResponse(bc: bare.ByteCursor, x: ToEnvoySqliteExecuteBatchResponse): void {
    bare.writeU32(bc, x.requestId)
    writeSqliteExecuteBatchResponse(bc, x.data)
}

export type ToEnvoy =
    | { readonly tag: "ToEnvoyInit"; readonly val: ToEnvoyInit }
    | { readonly tag: "ToEnvoyCommands"; readonly val: ToEnvoyCommands }
    | { readonly tag: "ToEnvoyAckEvents"; readonly val: ToEnvoyAckEvents }
    | { readonly tag: "ToEnvoyKvResponse"; readonly val: ToEnvoyKvResponse }
    | { readonly tag: "ToEnvoyTunnelMessage"; readonly val: ToEnvoyTunnelMessage }
    | { readonly tag: "ToEnvoyPing"; readonly val: ToEnvoyPing }
    | { readonly tag: "ToEnvoySqliteGetPagesResponse"; readonly val: ToEnvoySqliteGetPagesResponse }
    | { readonly tag: "ToEnvoySqliteCommitResponse"; readonly val: ToEnvoySqliteCommitResponse }
    | { readonly tag: "ToEnvoySqliteExecResponse"; readonly val: ToEnvoySqliteExecResponse }
    | { readonly tag: "ToEnvoySqliteExecuteResponse"; readonly val: ToEnvoySqliteExecuteResponse }
    | { readonly tag: "ToEnvoySqliteExecuteBatchResponse"; readonly val: ToEnvoySqliteExecuteBatchResponse }

export function readToEnvoy(bc: bare.ByteCursor): ToEnvoy {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "ToEnvoyInit", val: readToEnvoyInit(bc) }
        case 1:
            return { tag: "ToEnvoyCommands", val: readToEnvoyCommands(bc) }
        case 2:
            return { tag: "ToEnvoyAckEvents", val: readToEnvoyAckEvents(bc) }
        case 3:
            return { tag: "ToEnvoyKvResponse", val: readToEnvoyKvResponse(bc) }
        case 4:
            return { tag: "ToEnvoyTunnelMessage", val: readToEnvoyTunnelMessage(bc) }
        case 5:
            return { tag: "ToEnvoyPing", val: readToEnvoyPing(bc) }
        case 6:
            return { tag: "ToEnvoySqliteGetPagesResponse", val: readToEnvoySqliteGetPagesResponse(bc) }
        case 7:
            return { tag: "ToEnvoySqliteCommitResponse", val: readToEnvoySqliteCommitResponse(bc) }
        case 8:
            return { tag: "ToEnvoySqliteExecResponse", val: readToEnvoySqliteExecResponse(bc) }
        case 9:
            return { tag: "ToEnvoySqliteExecuteResponse", val: readToEnvoySqliteExecuteResponse(bc) }
        case 10:
            return { tag: "ToEnvoySqliteExecuteBatchResponse", val: readToEnvoySqliteExecuteBatchResponse(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeToEnvoy(bc: bare.ByteCursor, x: ToEnvoy): void {
    switch (x.tag) {
        case "ToEnvoyInit": {
            bare.writeU8(bc, 0)
            writeToEnvoyInit(bc, x.val)
            break
        }
        case "ToEnvoyCommands": {
            bare.writeU8(bc, 1)
            writeToEnvoyCommands(bc, x.val)
            break
        }
        case "ToEnvoyAckEvents": {
            bare.writeU8(bc, 2)
            writeToEnvoyAckEvents(bc, x.val)
            break
        }
        case "ToEnvoyKvResponse": {
            bare.writeU8(bc, 3)
            writeToEnvoyKvResponse(bc, x.val)
            break
        }
        case "ToEnvoyTunnelMessage": {
            bare.writeU8(bc, 4)
            writeToEnvoyTunnelMessage(bc, x.val)
            break
        }
        case "ToEnvoyPing": {
            bare.writeU8(bc, 5)
            writeToEnvoyPing(bc, x.val)
            break
        }
        case "ToEnvoySqliteGetPagesResponse": {
            bare.writeU8(bc, 6)
            writeToEnvoySqliteGetPagesResponse(bc, x.val)
            break
        }
        case "ToEnvoySqliteCommitResponse": {
            bare.writeU8(bc, 7)
            writeToEnvoySqliteCommitResponse(bc, x.val)
            break
        }
        case "ToEnvoySqliteExecResponse": {
            bare.writeU8(bc, 8)
            writeToEnvoySqliteExecResponse(bc, x.val)
            break
        }
        case "ToEnvoySqliteExecuteResponse": {
            bare.writeU8(bc, 9)
            writeToEnvoySqliteExecuteResponse(bc, x.val)
            break
        }
        case "ToEnvoySqliteExecuteBatchResponse": {
            bare.writeU8(bc, 10)
            writeToEnvoySqliteExecuteBatchResponse(bc, x.val)
            break
        }
    }
}

export function encodeToEnvoy(x: ToEnvoy, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeToEnvoy(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeToEnvoy(bytes: Uint8Array): ToEnvoy {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readToEnvoy(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}

/**
 * MARK: To Envoy Conn
 */
export type ToEnvoyConnPing = {
    readonly gatewayId: GatewayId
    readonly requestId: RequestId
    readonly ts: i64
}

export function readToEnvoyConnPing(bc: bare.ByteCursor): ToEnvoyConnPing {
    return {
        gatewayId: readGatewayId(bc),
        requestId: readRequestId(bc),
        ts: bare.readI64(bc),
    }
}

export function writeToEnvoyConnPing(bc: bare.ByteCursor, x: ToEnvoyConnPing): void {
    writeGatewayId(bc, x.gatewayId)
    writeRequestId(bc, x.requestId)
    bare.writeI64(bc, x.ts)
}

export type ToEnvoyConnClose = null

export type ToEnvoyConn =
    | { readonly tag: "ToEnvoyConnPing"; readonly val: ToEnvoyConnPing }
    | { readonly tag: "ToEnvoyConnClose"; readonly val: ToEnvoyConnClose }
    | { readonly tag: "ToEnvoyCommands"; readonly val: ToEnvoyCommands }
    | { readonly tag: "ToEnvoyAckEvents"; readonly val: ToEnvoyAckEvents }
    | { readonly tag: "ToEnvoyTunnelMessage"; readonly val: ToEnvoyTunnelMessage }

export function readToEnvoyConn(bc: bare.ByteCursor): ToEnvoyConn {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "ToEnvoyConnPing", val: readToEnvoyConnPing(bc) }
        case 1:
            return { tag: "ToEnvoyConnClose", val: null }
        case 2:
            return { tag: "ToEnvoyCommands", val: readToEnvoyCommands(bc) }
        case 3:
            return { tag: "ToEnvoyAckEvents", val: readToEnvoyAckEvents(bc) }
        case 4:
            return { tag: "ToEnvoyTunnelMessage", val: readToEnvoyTunnelMessage(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeToEnvoyConn(bc: bare.ByteCursor, x: ToEnvoyConn): void {
    switch (x.tag) {
        case "ToEnvoyConnPing": {
            bare.writeU8(bc, 0)
            writeToEnvoyConnPing(bc, x.val)
            break
        }
        case "ToEnvoyConnClose": {
            bare.writeU8(bc, 1)
            break
        }
        case "ToEnvoyCommands": {
            bare.writeU8(bc, 2)
            writeToEnvoyCommands(bc, x.val)
            break
        }
        case "ToEnvoyAckEvents": {
            bare.writeU8(bc, 3)
            writeToEnvoyAckEvents(bc, x.val)
            break
        }
        case "ToEnvoyTunnelMessage": {
            bare.writeU8(bc, 4)
            writeToEnvoyTunnelMessage(bc, x.val)
            break
        }
    }
}

export function encodeToEnvoyConn(x: ToEnvoyConn, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeToEnvoyConn(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeToEnvoyConn(bytes: Uint8Array): ToEnvoyConn {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readToEnvoyConn(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}

/**
 * MARK: To Gateway
 */
export type ToGatewayPong = {
    readonly requestId: RequestId
    readonly ts: i64
}

export function readToGatewayPong(bc: bare.ByteCursor): ToGatewayPong {
    return {
        requestId: readRequestId(bc),
        ts: bare.readI64(bc),
    }
}

export function writeToGatewayPong(bc: bare.ByteCursor, x: ToGatewayPong): void {
    writeRequestId(bc, x.requestId)
    bare.writeI64(bc, x.ts)
}

export type ToGateway =
    | { readonly tag: "ToGatewayPong"; readonly val: ToGatewayPong }
    | { readonly tag: "ToRivetTunnelMessage"; readonly val: ToRivetTunnelMessage }

export function readToGateway(bc: bare.ByteCursor): ToGateway {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "ToGatewayPong", val: readToGatewayPong(bc) }
        case 1:
            return { tag: "ToRivetTunnelMessage", val: readToRivetTunnelMessage(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeToGateway(bc: bare.ByteCursor, x: ToGateway): void {
    switch (x.tag) {
        case "ToGatewayPong": {
            bare.writeU8(bc, 0)
            writeToGatewayPong(bc, x.val)
            break
        }
        case "ToRivetTunnelMessage": {
            bare.writeU8(bc, 1)
            writeToRivetTunnelMessage(bc, x.val)
            break
        }
    }
}

export function encodeToGateway(x: ToGateway, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeToGateway(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeToGateway(bytes: Uint8Array): ToGateway {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readToGateway(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}

/**
 * MARK: To Outbound
 */
export type ToOutboundActorStart = {
    readonly namespaceId: Id
    readonly poolName: string
    readonly checkpoint: ActorCheckpoint
    readonly actorConfig: ActorConfig
}

export function readToOutboundActorStart(bc: bare.ByteCursor): ToOutboundActorStart {
    return {
        namespaceId: readId(bc),
        poolName: bare.readString(bc),
        checkpoint: readActorCheckpoint(bc),
        actorConfig: readActorConfig(bc),
    }
}

export function writeToOutboundActorStart(bc: bare.ByteCursor, x: ToOutboundActorStart): void {
    writeId(bc, x.namespaceId)
    bare.writeString(bc, x.poolName)
    writeActorCheckpoint(bc, x.checkpoint)
    writeActorConfig(bc, x.actorConfig)
}

export type ToOutbound =
    | { readonly tag: "ToOutboundActorStart"; readonly val: ToOutboundActorStart }

export function readToOutbound(bc: bare.ByteCursor): ToOutbound {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "ToOutboundActorStart", val: readToOutboundActorStart(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeToOutbound(bc: bare.ByteCursor, x: ToOutbound): void {
    switch (x.tag) {
        case "ToOutboundActorStart": {
            bare.writeU8(bc, 0)
            writeToOutboundActorStart(bc, x.val)
            break
        }
    }
}

export function encodeToOutbound(x: ToOutbound, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeToOutbound(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeToOutbound(bytes: Uint8Array): ToOutbound {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readToOutbound(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}


function assert(condition: boolean, message?: string): asserts condition {
    if (!condition) throw new Error(message ?? "Assertion failed")
}

export const VERSION = 8;