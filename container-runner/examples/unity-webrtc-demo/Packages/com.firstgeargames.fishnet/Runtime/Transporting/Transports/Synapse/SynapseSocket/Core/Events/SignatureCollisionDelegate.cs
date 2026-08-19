namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Raised when two connections produce the same 64-bit signature.
    /// The newer connection overwrites the reverse-lookup slot.
    /// </summary>
    /// <param name="signature">The 64-bit signature that two distinct endpoints produced.</param>
    public delegate void SignatureCollisionDelegate(ulong signature);
}
