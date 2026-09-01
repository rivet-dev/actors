namespace FishNet.Transporting
{
    /// <summary>
    /// Channel which data is sent or received.
    /// </summary>
    public enum IPAddressType : byte
    {
        /// <summary>
        /// Address is IPv4.
        /// </summary>
        IPv4 = 0,
        /// <summary>
        /// Address is IPv6.
        /// </summary>
        IPv6 = 1
    }
}