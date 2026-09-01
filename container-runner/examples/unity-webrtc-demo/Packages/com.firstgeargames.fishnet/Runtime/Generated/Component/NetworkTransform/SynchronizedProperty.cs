namespace FishNet.Component.Transforming
{
    [System.Flags]
    public enum SynchronizedProperty : byte
    {
        None = 0,
        Parent = 1,
        Position = 2,
        Rotation = 4,
        Scale = 8
    }
}