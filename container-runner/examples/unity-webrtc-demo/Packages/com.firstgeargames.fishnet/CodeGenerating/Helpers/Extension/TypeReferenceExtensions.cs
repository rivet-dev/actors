using MonoFN.Cecil;
using MonoFN.Cecil.Rocks;
using System;
using System.Collections.Generic;
using UnityEngine;

namespace FishNet.CodeGenerating.Helping.Extension
{
    internal static class TypeReferenceExtensionsOld
    {
        /// <summary>
        /// Gets a Resolve favoring cached results first.
        /// </summary>
        internal static TypeDefinition CachedResolve(this TypeReference typeRef, CodegenSession session)
        {
            return session.GetClass<GeneralHelper>().GetTypeReferenceResolve(typeRef);
        }

        /// <summary>
        /// Returns if typeRef is a class or struct.
        /// </summary>
        internal static bool IsClassOrStruct(this TypeReference typeRef, CodegenSession session)
        {
            TypeDefinition typeDef = typeRef.CachedResolve(session);
            return !typeDef.IsPrimitive && !typeDef.IsEnum && (typeDef.IsClass || typeDef.IsValueType);
        }

        /// <summary>
        /// Returns all properties on typeRef and all base types which have a public get/set accessor.
        /// </summary>
        /// <param name = "typeRef"></param>
        /// <returns></returns>
        public static IEnumerable<PropertyDefinition> FindAllSerializableProperties(this TypeReference typeRef, CodegenSession session, Type[] excludedBaseTypes = null, string[] excludedAssemblyPrefixes = null)
        {
            return typeRef.CachedResolve(session).FindAllPublicProperties(session, excludedBaseTypes, excludedAssemblyPrefixes);
        }

        /// <summary>
        /// Gets all public fields in typeRef and base type.
        /// </summary>
        /// <param name = "typeRef"></param>
        /// <returns></returns>
        public static IEnumerable<FieldDefinition> FindAllSerializableFields(this TypeReference typeRef, CodegenSession session, Type[] excludedBaseTypes = null, string[] excludedAssemblyPrefixes = null)
        {
            return typeRef.Resolve().FindAllPublicFields(session, excludedBaseTypes, excludedAssemblyPrefixes);
        }

        /// <summary>
        /// Returns if a typeRef is type.
        /// </summary>
        /// <param name = "typeRef"></param>
        /// <param name = "type"></param>
        /// <returns></returns>
        public static bool IsType(this TypeReference typeRef, Type type)
        {
            if (type.IsGenericType)
                return typeRef.GetElementType().FullName == type.FullName;
            else
                return typeRef.FullName == type.FullName;
        }

        /// <summary>
        /// Returns if typeRef is a multidimensional array.
        /// </summary>
        /// <param name = "typeRef"></param>
        /// <returns></returns>
        public static bool IsMultidimensionalArray(this TypeReference typeRef)
        {
            if (typeRef is ArrayType && typeRef.Name.Contains("[][]"))
                return true;

            return false;
        }

        /// <summary>
        /// Returns if typeRef can be resolved.
        /// </summary>
        /// <param name = "typeRef"></param>
        /// <returns></returns>
        public static bool CanBeResolved(this TypeReference typeRef, CodegenSession session)
        {
            while (typeRef != null)
            {
                if (typeRef.Scope.Name == "Windows")
                {
                    return false;
                }

                if (typeRef.Scope.Name == "mscorlib")
                {
                    TypeDefinition resolved = typeRef.CachedResolve(session);
                    return resolved != null;
                }

                try
                {
                    typeRef = typeRef.CachedResolve(session).BaseType;
                }
                catch
                {
                    return false;
                }
            }
            return true;
        }

        /// <summary>
        /// Creates a generic type out of another type, if needed.
        /// </summary>
        /// <param name = "type"></param>
        /// <returns></returns>
        public static TypeReference ConvertToGenericIfNeeded(this TypeDefinition type)
        {
            if (type.HasGenericParameters)
            {
                // get all the generic parameters and make a generic instance out of it
                TypeReference[] genericTypes = new TypeReference[type.GenericParameters.Count];
                for (int i = 0; i < type.GenericParameters.Count; i++)
                {
                    genericTypes[i] = type.GenericParameters[i].GetElementType();
                }

                return type.MakeGenericInstanceType(genericTypes);
            }
            else
            {
                return type;
            }
        }
    }
}