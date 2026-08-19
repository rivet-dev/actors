using FishNet.CodeGenerating.Helping.Extension;
using MonoFN.Cecil;
using System;
using System.Collections.Generic;
using UnityEngine;

namespace FishNet.CodeGenerating.Helping
{
    internal static class Constructors
    {
        /// <summary>
        /// Gets the first constructor that optionally has, or doesn't have parameters.
        /// </summary>
        /// <param name = "typeRef"></param>
        /// <returns></returns>
        public static MethodDefinition GetFirstConstructor(this TypeReference typeRef, CodegenSession session, bool requireParameters)
        {
            return typeRef.CachedResolve(session).GetFirstConstructor(requireParameters);
        }

        /// <summary>
        /// Gets the first constructor that optionally has, or doesn't have parameters.
        /// </summary>
        /// <param name = "typeRef"></param>
        /// <returns></returns>
        public static MethodDefinition GetFirstConstructor(this TypeDefinition typeDef, bool requireParameters)
        {
            foreach (MethodDefinition methodDef in typeDef.Methods)
            {
                if (methodDef.IsConstructor && methodDef.IsPublic)
                {
                    if (requireParameters && methodDef.Parameters.Count > 0)
                        return methodDef;
                    else if (!requireParameters && methodDef.Parameters.Count == 0)
                        return methodDef;
                }
            }

            return null;
        }

        /// <summary>
        /// Gets the first public constructor with no parameters.
        /// </summary>
        /// <returns></returns>
        public static MethodDefinition GetDefaultConstructor(this TypeReference typeRef, CodegenSession session)
        {
            return typeRef.CachedResolve(session).GetDefaultConstructor();
        }

        /// <summary>
        /// Gets the first public constructor with no parameters.
        /// </summary>
        /// <returns></returns>
        public static MethodDefinition GetDefaultConstructor(this TypeDefinition typeDef)
        {
            foreach (MethodDefinition methodDef in typeDef.Methods)
            {
                if (methodDef.IsConstructor && methodDef.IsPublic && methodDef.Parameters.Count == 0)
                    return methodDef;
            }

            return null;
        }

        /// <summary>
        /// Gets all constructors on typeDef.
        /// </summary>
        /// <returns></returns>
        public static List<MethodDefinition> GetConstructors(this TypeDefinition typeDef)
        {
            List<MethodDefinition> lst = new();
            foreach (MethodDefinition methodDef in typeDef.Methods)
            {
                if (methodDef.IsConstructor)
                    lst.Add(methodDef);
            }

            return lst;
        }

        /// <summary>
        /// Gets constructor which has arguments.
        /// </summary>
        /// <param name = "typeRef"></param>
        /// <returns></returns>
        public static MethodDefinition GetConstructor(this TypeReference typeRef, CodegenSession session, Type[] arguments)
        {
            return typeRef.CachedResolve(session).GetConstructor(arguments);
        }

        /// <summary>
        /// Gets constructor which has arguments.
        /// </summary>
        /// <param name = "typeDef"></param>
        /// <returns></returns>
        public static MethodDefinition GetConstructor(this TypeDefinition typeDef, Type[] arguments)
        {
            Type[] argsCopy = arguments == null ? new Type[0] : arguments;
            foreach (MethodDefinition methodDef in typeDef.Methods)
            {
                if (methodDef.IsConstructor && methodDef.IsPublic && methodDef.Parameters.Count == argsCopy.Length)
                {
                    bool match = true;
                    for (int i = 0; i < argsCopy.Length; i++)
                    {
                        if (methodDef.Parameters[0].ParameterType.FullName != argsCopy[i].FullName)
                        {
                            match = false;
                            break;
                        }
                    }

                    if (match)
                        return methodDef;
                }
            }
            return null;
        }

        /// <summary>
        /// Gets constructor which has arguments.
        /// </summary>
        /// <param name = "typeRef"></param>
        /// <returns></returns>
        public static MethodDefinition GetConstructor(this TypeReference typeRef, CodegenSession session, TypeReference[] arguments)
        {
            return typeRef.CachedResolve(session).GetConstructor(arguments);
        }

        /// <summary>
        /// Gets constructor which has arguments.
        /// </summary>
        /// <param name = "typeDef"></param>
        /// <returns></returns>
        public static MethodDefinition GetConstructor(this TypeDefinition typeDef, TypeReference[] arguments)
        {
            TypeReference[] argsCopy = arguments == null ? new TypeReference[0] : arguments;
            foreach (MethodDefinition methodDef in typeDef.Methods)
            {
                if (methodDef.IsConstructor && methodDef.IsPublic && methodDef.Parameters.Count == argsCopy.Length)
                {
                    bool match = true;
                    for (int i = 0; i < argsCopy.Length; i++)
                    {
                        if (methodDef.Parameters[0].ParameterType.FullName != argsCopy[i].FullName)
                        {
                            match = false;
                            break;
                        }
                    }

                    if (match)
                        return methodDef;
                }
            }
            return null;
        }

        /// <summary>
        /// Resolves the constructor with parameterCount for typeRef.
        /// </summary>
        /// <param name = "typeRef"></param>
        /// <returns></returns>
        public static MethodDefinition GetConstructor(this TypeReference typeRef, CodegenSession session, int parameterCount)
        {
            return typeRef.CachedResolve(session).GetConstructor(parameterCount);
        }

        /// <summary>
        /// Resolves the constructor with parameterCount for typeRef.
        /// </summary>
        /// <param name = "typeDef"></param>
        /// <returns></returns>
        public static MethodDefinition GetConstructor(this TypeDefinition typeDef, int parameterCount)
        {
            foreach (MethodDefinition methodDef in typeDef.Methods)
            {
                if (methodDef.IsConstructor && methodDef.IsPublic && methodDef.Parameters.Count == parameterCount)
                    return methodDef;
            }
            return null;
        }
    }
}