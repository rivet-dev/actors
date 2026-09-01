using System.Collections.Generic;
using System.Net;
using System.Diagnostics.CodeAnalysis;

namespace SynapseSocket.Core
{

    public class IPEndPointComparer : IEqualityComparer<IPEndPoint>
    {
        public static readonly IPEndPointComparer Default = new();
        
        public bool Equals(IPEndPoint? x, IPEndPoint? y)
        {
            if (x is null || y is null)
                return ReferenceEquals(x, y);

            return x.Port == y.Port && x.Address.Equals(y.Address);
        }

        public int GetHashCode([DisallowNull] IPEndPoint obj)
        {
            return unchecked(obj.Address.GetHashCode() * 397) ^ obj.Port;
        }
    }
}
