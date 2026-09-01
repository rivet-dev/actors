namespace FishNet.Observing
{
    /// <summary>
    /// States which observer(s) can change to.
    /// </summary>
    internal enum ObserverStateChange : byte
    {
        Unchanged = 0,
        Added = 1,
        Removed = 2
    }
}