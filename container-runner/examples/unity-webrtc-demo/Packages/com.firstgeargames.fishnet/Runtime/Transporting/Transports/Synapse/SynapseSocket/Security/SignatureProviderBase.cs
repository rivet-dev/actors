using System.Net;
using SynapseSocket.Security;
using UnityEngine;

namespace FishNet.Transporting.Synapse
{

    /// <summary>
    /// ScriptableObject base class for custom signature providers.
    /// Subclass this, create an asset, and assign it to <see cref="SynapseSocket.Core.Configuration.SecurityConfig.SignatureProviderAsset"/>
    /// in the inspector to override the default FNV-1a endpoint hash.
    /// </summary>
    public abstract class SignatureProviderBase : ScriptableObject, ISignatureProvider
    {
        /// <inheritdoc/>
        public abstract bool TryCompute(IPEndPoint endPoint, System.ReadOnlySpan<byte> handshakePayload, out ulong signature);
    }
}
