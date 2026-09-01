using System;
using System.Net;
using SynapseSocket.Security;
using UnityEngine;

namespace FishNet.Transporting.Synapse
{

    /// <summary>
    /// ScriptableObject base class for custom signature validators.
    /// Subclass this, create an asset, and assign it to <see cref="SynapseSocket.Core.Configuration.SecurityConfig.SignatureValidatorAsset"/>
    /// in the inspector to enforce allow-lists or application-specific identity checks during handshake.
    /// </summary>
    public abstract class SignatureValidatorBase : ScriptableObject, ISignatureValidator
    {
        /// <inheritdoc/>
        public abstract bool Validate(IPEndPoint endPoint, ulong signature, ReadOnlySpan<byte> handshakePayload);
    }
}
