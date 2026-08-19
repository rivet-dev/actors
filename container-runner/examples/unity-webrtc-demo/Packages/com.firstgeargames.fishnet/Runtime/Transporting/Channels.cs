namespace FishNet.Transporting
{
    /// <summary>
    /// Channel which data is sent or received.
    /// </summary>
    public enum Channel : byte
    {
        /// <summary>
        /// Data will be sent ordered reliable.
        /// </summary>
        Reliable = 0,
        /// <summary>
        /// Data will be sent unreliable.
        /// </summary>
        Unreliable = 1
    }
}