using UnityEngine;

namespace FishNet.Object.Synchronizing
{
    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class FloatSyncVar : SyncVar<float>, ICustomSync
    {
        public object GetSerializedType() => typeof(float);
        protected override float Interpolate(float previous, float current, float percent) => Mathf.Lerp(previous, current, percent);
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class DoubleSyncVar : SyncVar<double>, ICustomSync
    {
        public object GetSerializedType() => typeof(double);

        protected override double Interpolate(double previous, double current, float percent)
        {
            float a = (float)previous;
            float b = (float)current;
            return Mathf.Lerp(a, b, percent);
        }
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class SbyteSyncVar : SyncVar<sbyte>, ICustomSync
    {
        public object GetSerializedType() => typeof(sbyte);
        protected override sbyte Interpolate(sbyte previous, sbyte current, float percent) => (sbyte)Mathf.Lerp(previous, current, percent);
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class ByteSyncVar : SyncVar<byte>, ICustomSync
    {
        public object GetSerializedType() => typeof(byte);
        protected override byte Interpolate(byte previous, byte current, float percent) => (byte)Mathf.Lerp(previous, current, percent);
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class ShortSyncVar : SyncVar<short>, ICustomSync
    {
        public object GetSerializedType() => typeof(short);
        protected override short Interpolate(short previous, short current, float percent) => (short)Mathf.Lerp(previous, current, percent);
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class UShortSyncVar : SyncVar<ushort>, ICustomSync
    {
        public object GetSerializedType() => typeof(ushort);
        protected override ushort Interpolate(ushort previous, ushort current, float percent) => (ushort)Mathf.Lerp(previous, current, percent);
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class IntSyncVar : SyncVar<int>, ICustomSync
    {
        public object GetSerializedType() => typeof(int);
        protected override int Interpolate(int previous, int current, float percent) => (int)Mathf.Lerp(previous, current, percent);
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class UIntSyncVar : SyncVar<uint>, ICustomSync
    {
        public object GetSerializedType() => typeof(uint);
        protected override uint Interpolate(uint previous, uint current, float percent) => (uint)Mathf.Lerp(previous, current, percent);
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class LongSyncVar : SyncVar<long>, ICustomSync
    {
        public object GetSerializedType() => typeof(long);
        protected override long Interpolate(long previous, long current, float percent) => (long)Mathf.Lerp(previous, current, percent);
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class ULongSyncVar : SyncVar<ulong>, ICustomSync
    {
        public object GetSerializedType() => typeof(ulong);
        protected override ulong Interpolate(ulong previous, ulong current, float percent) => (ulong)Mathf.Lerp(previous, current, percent);
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class Vector2SyncVar : SyncVar<Vector2>, ICustomSync
    {
        public object GetSerializedType() => typeof(Vector2);
        protected override Vector2 Interpolate(Vector2 previous, Vector2 current, float percent) => Vector2.Lerp(previous, current, percent);
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class Vector3SyncVar : SyncVar<Vector3>, ICustomSync
    {
        public object GetSerializedType() => typeof(Vector3);
        protected override Vector3 Interpolate(Vector3 previous, Vector3 current, float percent) => Vector3.Lerp(previous, current, percent);
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class Vector4SyncVar : SyncVar<Vector4>, ICustomSync
    {
        public object GetSerializedType() => typeof(Vector4);
        protected override Vector4 Interpolate(Vector4 previous, Vector4 current, float percent) => Vector4.Lerp(previous, current, percent);
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class Vector2IntSyncVar : SyncVar<Vector2Int>, ICustomSync
    {
        public object GetSerializedType() => typeof(Vector2);

        protected override Vector2Int Interpolate(Vector2Int previous, Vector2Int current, float percent)
        {
            int x = (int)Mathf.Lerp(previous.x, current.x, percent);
            int y = (int)Mathf.Lerp(previous.y, current.y, percent);
            return new(x, y);
        }
    }

    /// <summary>
    /// Implements features specific for a typed SyncVar.
    /// </summary>
    [System.Serializable]
    public class Vector3IntSyncVar : SyncVar<Vector3Int>, ICustomSync
    {
        public object GetSerializedType() => typeof(Vector3Int);

        protected override Vector3Int Interpolate(Vector3Int previous, Vector3Int current, float percent)
        {
            int x = (int)Mathf.Lerp(previous.x, current.x, percent);
            int y = (int)Mathf.Lerp(previous.y, current.y, percent);
            int z = (int)Mathf.Lerp(previous.z, current.z, percent);
            return new(x, y, z);
        }
    }
}