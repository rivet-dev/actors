using MonoFN.Cecil;
using System.Collections.Generic;

namespace FishNet.CodeGenerating.Helping
{
    internal class TypeDefinitionComparer : IEqualityComparer<TypeDefinition>
    {
        public bool Equals(TypeDefinition a, TypeDefinition b)
        {
            return a.FullName == b.FullName;
        }

        public int GetHashCode(TypeDefinition obj)
        {
            return obj.FullName.GetHashCode();
        }
    }

    internal class TypeReferenceComparer : IEqualityComparer<TypeReference>
    {
        public bool Equals(TypeReference a, TypeReference b)
        {
            return a.FullName == b.FullName;
        }

        public int GetHashCode(TypeReference obj)
        {
            return obj.FullName.GetHashCode();
        }
    }
}