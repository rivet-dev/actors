using MonoFN.Cecil.Cil;
using System;

namespace MonoFN.Cecil
{
    internal sealed class GenericParameterResolver
    {
        internal static TypeReference ResolveReturnTypeIfNeeded(MethodReference methodReference)
        {
            if (methodReference.DeclaringType.IsArray && methodReference.Name == "Get")
                return methodReference.ReturnType;

            GenericInstanceMethod genericInstanceMethod = methodReference as GenericInstanceMethod;
            GenericInstanceType declaringGenericInstanceType = methodReference.DeclaringType as GenericInstanceType;

            if (genericInstanceMethod == null && declaringGenericInstanceType == null)
                return methodReference.ReturnType;

            return ResolveIfNeeded(genericInstanceMethod, declaringGenericInstanceType, methodReference.ReturnType);
        }

        internal static TypeReference ResolveFieldTypeIfNeeded(FieldReference fieldReference)
        {
            return ResolveIfNeeded(null, fieldReference.DeclaringType as GenericInstanceType, fieldReference.FieldType);
        }

        internal static TypeReference ResolveParameterTypeIfNeeded(MethodReference method, ParameterReference parameter)
        {
            GenericInstanceMethod genericInstanceMethod = method as GenericInstanceMethod;
            GenericInstanceType declaringGenericInstanceType = method.DeclaringType as GenericInstanceType;

            if (genericInstanceMethod == null && declaringGenericInstanceType == null)
                return parameter.ParameterType;

            return ResolveIfNeeded(genericInstanceMethod, declaringGenericInstanceType, parameter.ParameterType);
        }

        internal static TypeReference ResolveVariableTypeIfNeeded(MethodReference method, VariableReference variable)
        {
            GenericInstanceMethod genericInstanceMethod = method as GenericInstanceMethod;
            GenericInstanceType declaringGenericInstanceType = method.DeclaringType as GenericInstanceType;

            if (genericInstanceMethod == null && declaringGenericInstanceType == null)
                return variable.VariableType;

            return ResolveIfNeeded(genericInstanceMethod, declaringGenericInstanceType, variable.VariableType);
        }

        private static TypeReference ResolveIfNeeded(IGenericInstance genericInstanceMethod, IGenericInstance declaringGenericInstanceType, TypeReference parameterType)
        {
            ByReferenceType byRefType = parameterType as ByReferenceType;
            if (byRefType != null)
                return ResolveIfNeeded(genericInstanceMethod, declaringGenericInstanceType, byRefType);

            ArrayType arrayType = parameterType as ArrayType;
            if (arrayType != null)
                return ResolveIfNeeded(genericInstanceMethod, declaringGenericInstanceType, arrayType);

            GenericInstanceType genericInstanceType = parameterType as GenericInstanceType;
            if (genericInstanceType != null)
                return ResolveIfNeeded(genericInstanceMethod, declaringGenericInstanceType, genericInstanceType);

            GenericParameter genericParameter = parameterType as GenericParameter;
            if (genericParameter != null)
                return ResolveIfNeeded(genericInstanceMethod, declaringGenericInstanceType, genericParameter);

            RequiredModifierType requiredModifierType = parameterType as RequiredModifierType;
            if (requiredModifierType != null && ContainsGenericParameters(requiredModifierType))
                return ResolveIfNeeded(genericInstanceMethod, declaringGenericInstanceType, requiredModifierType.ElementType);

            if (ContainsGenericParameters(parameterType))
                throw new("Unexpected generic parameter.");

            return parameterType;
        }

        private static TypeReference ResolveIfNeeded(IGenericInstance genericInstanceMethod, IGenericInstance genericInstanceType, GenericParameter genericParameterElement)
        {
            return genericParameterElement.MetadataType == MetadataType.MVar ? genericInstanceMethod != null ? genericInstanceMethod.GenericArguments[genericParameterElement.Position] : genericParameterElement : genericInstanceType.GenericArguments[genericParameterElement.Position];
        }

        private static ArrayType ResolveIfNeeded(IGenericInstance genericInstanceMethod, IGenericInstance genericInstanceType, ArrayType arrayType)
        {
            return new(ResolveIfNeeded(genericInstanceMethod, genericInstanceType, arrayType.ElementType), arrayType.Rank);
        }

        private static ByReferenceType ResolveIfNeeded(IGenericInstance genericInstanceMethod, IGenericInstance genericInstanceType, ByReferenceType byReferenceType)
        {
            return new(ResolveIfNeeded(genericInstanceMethod, genericInstanceType, byReferenceType.ElementType));
        }

        private static GenericInstanceType ResolveIfNeeded(IGenericInstance genericInstanceMethod, IGenericInstance genericInstanceType, GenericInstanceType genericInstanceType1)
        {
            if (!ContainsGenericParameters(genericInstanceType1))
                return genericInstanceType1;

            GenericInstanceType newGenericInstance = new(genericInstanceType1.ElementType);

            foreach (TypeReference genericArgument in genericInstanceType1.GenericArguments)
            {
                if (!genericArgument.IsGenericParameter)
                {
                    newGenericInstance.GenericArguments.Add(ResolveIfNeeded(genericInstanceMethod, genericInstanceType, genericArgument));
                    continue;
                }

                GenericParameter genParam = (GenericParameter)genericArgument;

                switch (genParam.Type)
                {
                    case GenericParameterType.Type:
                    {
                        if (genericInstanceType == null)
                            throw new NotSupportedException();

                        newGenericInstance.GenericArguments.Add(genericInstanceType.GenericArguments[genParam.Position]);
                    }
                        break;

                    case GenericParameterType.Method:
                    {
                        if (genericInstanceMethod == null)
                            newGenericInstance.GenericArguments.Add(genParam);
                        else
                            newGenericInstance.GenericArguments.Add(genericInstanceMethod.GenericArguments[genParam.Position]);
                    }
                        break;
                }
            }

            return newGenericInstance;
        }

        private static bool ContainsGenericParameters(TypeReference typeReference)
        {
            GenericParameter genericParameter = typeReference as GenericParameter;
            if (genericParameter != null)
                return true;

            ArrayType arrayType = typeReference as ArrayType;
            if (arrayType != null)
                return ContainsGenericParameters(arrayType.ElementType);

            PointerType pointerType = typeReference as PointerType;
            if (pointerType != null)
                return ContainsGenericParameters(pointerType.ElementType);

            ByReferenceType byRefType = typeReference as ByReferenceType;
            if (byRefType != null)
                return ContainsGenericParameters(byRefType.ElementType);

            SentinelType sentinelType = typeReference as SentinelType;
            if (sentinelType != null)
                return ContainsGenericParameters(sentinelType.ElementType);

            PinnedType pinnedType = typeReference as PinnedType;
            if (pinnedType != null)
                return ContainsGenericParameters(pinnedType.ElementType);

            RequiredModifierType requiredModifierType = typeReference as RequiredModifierType;
            if (requiredModifierType != null)
                return ContainsGenericParameters(requiredModifierType.ElementType);

            GenericInstanceType genericInstance = typeReference as GenericInstanceType;
            if (genericInstance != null)
            {
                foreach (TypeReference genericArgument in genericInstance.GenericArguments)
                {
                    if (ContainsGenericParameters(genericArgument))
                        return true;
                }

                return false;
            }

            if (typeReference is TypeSpecification)
                throw new NotSupportedException();

            return false;
        }
    }
}