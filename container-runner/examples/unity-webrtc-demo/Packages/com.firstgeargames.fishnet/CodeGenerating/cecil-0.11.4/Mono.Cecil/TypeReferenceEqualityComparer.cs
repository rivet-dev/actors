using System;
using System.Collections.Generic;

namespace MonoFN.Cecil
{
    internal sealed class TypeReferenceEqualityComparer : EqualityComparer<TypeReference>
    {
        public override bool Equals(TypeReference x, TypeReference y)
        {
            return AreEqual(x, y);
        }

        public override int GetHashCode(TypeReference obj)
        {
            return GetHashCodeFor(obj);
        }

        public static bool AreEqual(TypeReference a, TypeReference b, TypeComparisonMode comparisonMode = TypeComparisonMode.Exact)
        {
            if (ReferenceEquals(a, b))
                return true;

            if (a == null || b == null)
                return false;

            MetadataType aMetadataType = a.MetadataType;
            MetadataType bMetadataType = b.MetadataType;

            if (aMetadataType == MetadataType.GenericInstance || bMetadataType == MetadataType.GenericInstance)
            {
                if (aMetadataType != bMetadataType)
                    return false;

                return AreEqual((GenericInstanceType)a, (GenericInstanceType)b, comparisonMode);
            }

            if (aMetadataType == MetadataType.Array || bMetadataType == MetadataType.Array)
            {
                if (aMetadataType != bMetadataType)
                    return false;

                ArrayType a1 = (ArrayType)a;
                ArrayType b1 = (ArrayType)b;
                if (a1.Rank != b1.Rank)
                    return false;

                return AreEqual(a1.ElementType, b1.ElementType, comparisonMode);
            }

            if (aMetadataType == MetadataType.Var || bMetadataType == MetadataType.Var)
            {
                if (aMetadataType != bMetadataType)
                    return false;

                return AreEqual((GenericParameter)a, (GenericParameter)b, comparisonMode);
            }

            if (aMetadataType == MetadataType.MVar || bMetadataType == MetadataType.MVar)
            {
                if (aMetadataType != bMetadataType)
                    return false;

                return AreEqual((GenericParameter)a, (GenericParameter)b, comparisonMode);
            }

            if (aMetadataType == MetadataType.ByReference || bMetadataType == MetadataType.ByReference)
            {
                if (aMetadataType != bMetadataType)
                    return false;

                return AreEqual(((ByReferenceType)a).ElementType, ((ByReferenceType)b).ElementType, comparisonMode);
            }

            if (aMetadataType == MetadataType.Pointer || bMetadataType == MetadataType.Pointer)
            {
                if (aMetadataType != bMetadataType)
                    return false;

                return AreEqual(((PointerType)a).ElementType, ((PointerType)b).ElementType, comparisonMode);
            }

            if (aMetadataType == MetadataType.RequiredModifier || bMetadataType == MetadataType.RequiredModifier)
            {
                if (aMetadataType != bMetadataType)
                    return false;

                RequiredModifierType a1 = (RequiredModifierType)a;
                RequiredModifierType b1 = (RequiredModifierType)b;

                return AreEqual(a1.ModifierType, b1.ModifierType, comparisonMode) && AreEqual(a1.ElementType, b1.ElementType, comparisonMode);
            }

            if (aMetadataType == MetadataType.OptionalModifier || bMetadataType == MetadataType.OptionalModifier)
            {
                if (aMetadataType != bMetadataType)
                    return false;

                OptionalModifierType a1 = (OptionalModifierType)a;
                OptionalModifierType b1 = (OptionalModifierType)b;

                return AreEqual(a1.ModifierType, b1.ModifierType, comparisonMode) && AreEqual(a1.ElementType, b1.ElementType, comparisonMode);
            }

            if (aMetadataType == MetadataType.Pinned || bMetadataType == MetadataType.Pinned)
            {
                if (aMetadataType != bMetadataType)
                    return false;

                return AreEqual(((PinnedType)a).ElementType, ((PinnedType)b).ElementType, comparisonMode);
            }

            if (aMetadataType == MetadataType.Sentinel || bMetadataType == MetadataType.Sentinel)
            {
                if (aMetadataType != bMetadataType)
                    return false;

                return AreEqual(((SentinelType)a).ElementType, ((SentinelType)b).ElementType, comparisonMode);
            }

            if (!a.Name.Equals(b.Name) || !a.Namespace.Equals(b.Namespace))
                return false;

            TypeDefinition xDefinition = a.Resolve();
            TypeDefinition yDefinition = b.Resolve();

            // For loose signature the types could be in different assemblies, as long as the type names match we will consider them equal
            if (comparisonMode == TypeComparisonMode.SignatureOnlyLoose)
            {
                if (xDefinition.Module.Name != yDefinition.Module.Name)
                    return false;

                if (xDefinition.Module.Assembly.Name.Name != yDefinition.Module.Assembly.Name.Name)
                    return false;

                return xDefinition.FullName == yDefinition.FullName;
            }

            return xDefinition == yDefinition;
        }

        private static bool AreEqual(GenericParameter a, GenericParameter b, TypeComparisonMode comparisonMode = TypeComparisonMode.Exact)
        {
            if (ReferenceEquals(a, b))
                return true;

            if (a.Position != b.Position)
                return false;

            if (a.Type != b.Type)
                return false;

            TypeReference aOwnerType = a.Owner as TypeReference;
            if (aOwnerType != null && AreEqual(aOwnerType, b.Owner as TypeReference, comparisonMode))
                return true;

            MethodReference aOwnerMethod = a.Owner as MethodReference;
            if (aOwnerMethod != null && comparisonMode != TypeComparisonMode.SignatureOnlyLoose && MethodReferenceComparer.AreEqual(aOwnerMethod, b.Owner as MethodReference))
                return true;

            return comparisonMode == TypeComparisonMode.SignatureOnly || comparisonMode == TypeComparisonMode.SignatureOnlyLoose;
        }

        private static bool AreEqual(GenericInstanceType a, GenericInstanceType b, TypeComparisonMode comparisonMode = TypeComparisonMode.Exact)
        {
            if (ReferenceEquals(a, b))
                return true;

            int aGenericArgumentsCount = a.GenericArguments.Count;
            if (aGenericArgumentsCount != b.GenericArguments.Count)
                return false;

            if (!AreEqual(a.ElementType, b.ElementType, comparisonMode))
                return false;

            for (int i = 0; i < aGenericArgumentsCount; i++)
                if (!AreEqual(a.GenericArguments[i], b.GenericArguments[i], comparisonMode))
                    return false;

            return true;
        }

        public static int GetHashCodeFor(TypeReference obj)
        {
            // a very good prime number
            const int hashCodeMultiplier = 486187739;
            // prime numbers
            const int genericInstanceTypeMultiplier = 31;
            const int byReferenceMultiplier = 37;
            const int pointerMultiplier = 41;
            const int requiredModifierMultiplier = 43;
            const int optionalModifierMultiplier = 47;
            const int pinnedMultiplier = 53;
            const int sentinelMultiplier = 59;

            MetadataType metadataType = obj.MetadataType;

            if (metadataType == MetadataType.GenericInstance)
            {
                GenericInstanceType genericInstanceType = (GenericInstanceType)obj;
                int hashCode = GetHashCodeFor(genericInstanceType.ElementType) * hashCodeMultiplier + genericInstanceTypeMultiplier;
                for (int i = 0; i < genericInstanceType.GenericArguments.Count; i++)
                    hashCode = hashCode * hashCodeMultiplier + GetHashCodeFor(genericInstanceType.GenericArguments[i]);
                return hashCode;
            }

            if (metadataType == MetadataType.Array)
            {
                ArrayType arrayType = (ArrayType)obj;
                return GetHashCodeFor(arrayType.ElementType) * hashCodeMultiplier + arrayType.Rank.GetHashCode();
            }

            if (metadataType == MetadataType.Var || metadataType == MetadataType.MVar)
            {
                GenericParameter genericParameter = (GenericParameter)obj;
                int hashCode = genericParameter.Position.GetHashCode() * hashCodeMultiplier + ((int)metadataType).GetHashCode();

                TypeReference ownerTypeReference = genericParameter.Owner as TypeReference;
                if (ownerTypeReference != null)
                    return hashCode * hashCodeMultiplier + GetHashCodeFor(ownerTypeReference);

                MethodReference ownerMethodReference = genericParameter.Owner as MethodReference;
                if (ownerMethodReference != null)
                    return hashCode * hashCodeMultiplier + MethodReferenceComparer.GetHashCodeFor(ownerMethodReference);

                throw new InvalidOperationException("Generic parameter encountered with invalid owner");
            }

            if (metadataType == MetadataType.ByReference)
            {
                ByReferenceType byReferenceType = (ByReferenceType)obj;
                return GetHashCodeFor(byReferenceType.ElementType) * hashCodeMultiplier * byReferenceMultiplier;
            }

            if (metadataType == MetadataType.Pointer)
            {
                PointerType pointerType = (PointerType)obj;
                return GetHashCodeFor(pointerType.ElementType) * hashCodeMultiplier * pointerMultiplier;
            }

            if (metadataType == MetadataType.RequiredModifier)
            {
                RequiredModifierType requiredModifierType = (RequiredModifierType)obj;
                int hashCode = GetHashCodeFor(requiredModifierType.ElementType) * requiredModifierMultiplier;
                hashCode = hashCode * hashCodeMultiplier + GetHashCodeFor(requiredModifierType.ModifierType);
                return hashCode;
            }

            if (metadataType == MetadataType.OptionalModifier)
            {
                OptionalModifierType optionalModifierType = (OptionalModifierType)obj;
                int hashCode = GetHashCodeFor(optionalModifierType.ElementType) * optionalModifierMultiplier;
                hashCode = hashCode * hashCodeMultiplier + GetHashCodeFor(optionalModifierType.ModifierType);
                return hashCode;
            }

            if (metadataType == MetadataType.Pinned)
            {
                PinnedType pinnedType = (PinnedType)obj;
                return GetHashCodeFor(pinnedType.ElementType) * hashCodeMultiplier * pinnedMultiplier;
            }

            if (metadataType == MetadataType.Sentinel)
            {
                SentinelType sentinelType = (SentinelType)obj;
                return GetHashCodeFor(sentinelType.ElementType) * hashCodeMultiplier * sentinelMultiplier;
            }

            if (metadataType == MetadataType.FunctionPointer)
            {
                throw new NotImplementedException("We currently don't handle function pointer types.");
            }

            return obj.Namespace.GetHashCode() * hashCodeMultiplier + obj.FullName.GetHashCode();
        }
    }
}