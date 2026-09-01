using UnityEngine;

namespace FishNet.Serializing.Helping
{
    public struct QuaternionAutoPack
    {
        public Quaternion Value;
        public AutoPackType PackType;
        
        public QuaternionAutoPack(Quaternion value)
        {
            Value = value;
            PackType = AutoPackType.Packed;
        }

        public QuaternionAutoPack(Quaternion value, AutoPackType autoPackType)
        {
            Value = value;
            PackType = autoPackType;
        }
        
    }

    public static class QuaternionAutoPackExtensions
    {
        public static void WriteQuaternionAutoPack(this Writer w, QuaternionAutoPack value)
        {
            w.WriteUInt8Unpacked((byte)value.PackType);
            w.WriteQuaternion(value.Value, value.PackType);
        }

        public static QuaternionAutoPack ReadUnpackedQuaternion(this Reader reader)
        {
            AutoPackType packType = (AutoPackType)reader.ReadUInt8Unpacked();
            Quaternion q = reader.ReadQuaternion(packType);
            
            return new(q, packType);
        }
    }
}
