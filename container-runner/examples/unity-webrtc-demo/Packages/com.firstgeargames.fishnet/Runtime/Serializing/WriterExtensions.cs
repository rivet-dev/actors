// using FishNet.CodeGenerating; // Remove on V5
// using FishNet.Connection;
// using FishNet.Documenting;
// using FishNet.Object;
// using FishNet.Serializing.Helping;
// using FishNet.Transporting;
// using System;
// using System.Collections.Generic;
// using UnityEngine;

// namespace FishNet.Serializing
// {

//    /// <summary>
//    /// Extensions to Write methods. Used by Write<T>.
//    /// </summary>
//    [APIExclude]
//    public static class WriterExtensions
//    {

//        /// <summary>
//        /// Types which are are set to auto pack by default.
//        /// </summary>
//        internal static HashSet<System.Type> DefaultPackedTypes = new HashSet<System.Type>();

//        static WriterExtensions()
//        {
//            DefaultPackedTypes.Add(typeof(short));
//            DefaultPackedTypes.Add(typeof(ushort));
//            DefaultPackedTypes.Add(typeof(int));
//            DefaultPackedTypes.Add(typeof(uint));
//            DefaultPackedTypes.Add(typeof(long));
//            DefaultPackedTypes.Add(typeof(ulong));
//            DefaultPackedTypes.Add(typeof(Color));
//            DefaultPackedTypes.Add(typeof(Quaternion));
//            DefaultPackedTypes.Add(typeof(Vector2Int));
//            DefaultPackedTypes.Add(typeof(Vector3Int));
//            DefaultPackedTypes.Add(typeof(Quaternion));
//        }

//        // public static void WriteDictionary<TKey, TValue>(this Writer writer, Dictionary<TKey, TValue> dict) => writer.WriteDictionary(dict);
//        // public static void WriteByte(this Writer writer, byte value) => writer.WriteByte(value);
//        //
//        // public static void WriteBytes(this Writer writer, byte[] buffer, int offset, int count) => writer.WriteBytes(buffer, offset, count);
//        //
//        // public static void WriteBytesAndSize(this Writer writer, byte[] buffer, int offset, int count) => writer.WriteBytesAndSize(buffer, offset, count);
//        // public static void WriteBytesAndSize(this Writer writer, byte[] value) => writer.WriteBytesAndSize(value);

//        // public static void WriteSByte(this Writer writer, sbyte value) => writer.WriteSByte(value);
//        // public static void WriteChar(this Writer writer, char value) => writer.WriteChar(value);
//        // public static void WriteBoolean(this Writer writer, bool value) => writer.WriteBoolean(value);
//        // public static void WriteUInt16(this Writer writer, ushort value) => writer.WriteUInt16(value);
//        // public static void WriteInt16(this Writer writer, short value) => writer.WriteInt16(value);
//        // public static void WriteInt32(this Writer writer, int value, AutoPackType packType = AutoPackType.Packed) => writer.WriteInt32(value, packType);
//        // public static void WriteUInt32(this Writer writer, uint value, AutoPackType packType = AutoPackType.Packed) => writer.WriteUInt32(value, packType);
//        // public static void WriteInt64(this Writer writer, long value, AutoPackType packType = AutoPackType.Packed) => writer.WriteInt64(value, packType);
//        // public static void WriteUInt64(this Writer writer, ulong value, AutoPackType packType = AutoPackType.Packed) => writer.WriteUInt64(value, packType);
//        // public static void WriteSingle(this Writer writer, float value, AutoPackType packType = AutoPackType.Unpacked) => writer.WriteSingle(value, packType);
//        // public static void WriteDouble(this Writer writer, double value) => writer.WriteDouble(value);
//        // public static void WriteDecimal(this Writer writer, decimal value) => writer.WriteDecimal(value);
//        // public static void WriteString(this Writer writer, string value) => writer.WriteString(value);
//        // public static void WriteArraySegmentAndSize(this Writer writer, ArraySegment<byte> value) => writer.WriteArraySegmentAndSize(value);
//        //
//        // public static void WriteArraySegment(this Writer writer, ArraySegment<byte> value) => writer.WriteArraySegment(value);
//        // public static void WriteVector2(this Writer writer, Vector2 value) => writer.WriteVector2(value);
//        // public static void WriteVector3(this Writer writer, Vector3 value) => writer.WriteVector3(value);
//        // public static void WriteVector4(this Writer writer, Vector4 value) => writer.WriteVector4(value);
//        // public static void WriteVector2Int(this Writer writer, Vector2Int value, AutoPackType packType = AutoPackType.Packed) => writer.WriteVector2Int(value, packType);
//        // public static void WriteVector3Int(this Writer writer, Vector3Int value, AutoPackType packType = AutoPackType.Packed) => writer.WriteVector3Int(value, packType);
//        // public static void WriteColor(this Writer writer, Color value, AutoPackType packType) => writer.WriteColor(value, packType);
//        // public static void WriteColor32(this Writer writer, Color32 value) => writer.WriteColor32(value);
//        // public static void WriteQuaternion(this Writer writer, Quaternion value, AutoPackType packType = AutoPackType.Packed) => writer.WriteQuaternion(value, packType);
//        // public static void WriteRect(this Writer writer, Rect value) => writer.WriteRect(value);
//        // public static void WritePlane(this Writer writer, Plane value) => writer.WritePlane(value);
//        // public static void WriteRay(this Writer writer, Ray value) => writer.WriteRay(value);
//        // public static void WriteRay2D(this Writer writer, Ray2D value) => writer.WriteRay2D(value);
//        // public static void WriteMatrix4x4(this Writer writer, Matrix4x4 value) => writer.WriteMatrix4x4(value);
//        // public static void WriteGuidAllocated(this Writer writer, System.Guid value) => writer.WriteGuidAllocated(value);
//        // public static void WriteGameObject(this Writer writer, GameObject value) => writer.WriteGameObject(value);
//        // public static void WriteTransform(this Writer writer, Transform value) => writer.WriteTransform(value);
//        // public static void WriteNetworkObject(this Writer writer, NetworkObject value) => writer.WriteNetworkObject(value);
//        // public static void WriteNetworkBehaviour(this Writer writer, NetworkBehaviour value) => writer.WriteNetworkBehaviour(value);
//        // public static void WriteChannel(this Writer writer, Channel value) => writer.WriteChannel(value);
//        // public static void WriteNetworkConnection(this Writer writer, NetworkConnection value) => writer.WriteNetworkConnection(value);
//        //
//        // public static void Write<T>(this Writer writer, T value) => writer.Write<T>(value);

//    }
// }

