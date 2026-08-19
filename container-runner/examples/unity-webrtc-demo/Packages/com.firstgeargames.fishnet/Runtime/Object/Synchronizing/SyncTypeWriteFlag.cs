namespace FishNet.Object.Synchronizing
{
    [System.Flags]
    internal enum SyncTypeWriteFlag
    {
        Unset = 0,
        IgnoreInterval = 1,
        ForceReliable = 2
    }

    internal static class SyncTypeWriteFlagExtensions
    {
        public static bool FastContains(this SyncTypeWriteFlag whole, SyncTypeWriteFlag part) => (whole & part) == part;
    }
}