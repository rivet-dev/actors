namespace FishNet.Serializing
{
    [System.Flags]
    public enum DeltaSerializerOption : ulong
    {
        Unset = 0,
        FullSerialize = 1,
        RootSerialize = 2
    }

    public static class DeltaSerializerOptionExtensions
    {
        public static bool FastContains(this DeltaSerializerOption whole, DeltaSerializerOption part) => (whole & part) == part;
    }
}