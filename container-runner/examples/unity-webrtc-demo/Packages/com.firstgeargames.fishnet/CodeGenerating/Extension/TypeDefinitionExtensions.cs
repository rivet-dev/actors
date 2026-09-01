using FishNet.CodeGenerating.Helping.Extension;
using FishNet.Object.Prediction;
using FishNet.Utility.Performance;
using MonoFN.Cecil;
using System.Collections.Generic;

namespace FishNet.CodeGenerating.Extension
{
    internal static class TypeDefinitionExtensions
    {
        /// <summary>
        /// Returns if a TypeDefinition is nullable.
        /// </summary>
        public static bool IsNullable(this TypeDefinition td)
        {
            return td.Name == typeof(System.Nullable<>).Name;
        }

        /// <summary>
        /// Finds the first method by a given name.
        /// </summary>
        /// <param name = "typeDef"></param>
        /// <param name = "methodName"></param>
        /// <returns></returns>
        internal static MethodDefinition GetMethod(this TypeDefinition typeDef, string methodName)
        {
            foreach (MethodDefinition md in typeDef.Methods)
            {
                if (md.Name == methodName)
                    return md;
            }

            return null;
        }

        public static MethodReference GetMethodReferenceInBase(this TypeDefinition td, CodegenSession session, string methodName)
        {
            MethodDefinition baseMd = td.GetMethodDefinitionInBase(session, methodName);
            if (baseMd == null)
                return null;

            MethodReference baseMr;
            TypeReference baseTr = td.BaseType;
            if (baseTr.CachedResolve(session).HasGenericParameters)
            {
                GenericInstanceType git = (GenericInstanceType)baseTr;
                baseMr = new(baseMd.Name, baseMd.ReturnType, git)
                {
                    HasThis = baseMd.HasThis,
                    CallingConvention = baseMd.CallingConvention,
                    ExplicitThis = baseMd.ExplicitThis
                };
                foreach (ParameterDefinition pd in baseMd.Parameters)
                    baseMr.Parameters.Add(pd.CloneImported(session, baseMr));
            }
            else
            {
                baseMr = session.ImportReference(baseMd);
            }

            return baseMr;
        }

        /// <summary>
        /// Returns a method in the next base class.
        /// </summary>
        public static MethodDefinition GetMethodDefinitionInBase(this TypeDefinition td, CodegenSession session, string methodName)
        {
            if (td.BaseType == null)
            {
                session.LogError($"BaseType for {td.FullName} is null.");
                return null;
            }

            TypeDefinition baseTd = td.BaseType.CachedResolve(session);
            return baseTd.GetMethod(methodName);
        }

        /// <summary>
        /// Returns a method in the next base class.
        /// </summary>
        public static MethodReference GetMethodReference(this TypeDefinition td, CodegenSession session, string methodName)
        {
            MethodDefinition md = td.GetMethod(methodName);
            // Not found.
            if (md == null)
                return null;

            return md.GetMethodReference(session);
        }

        /// <summary>
        /// Gets a MethodReference or creates one if missing.
        /// </summary>
        public static MethodReference GetOrCreateMethodReference(this TypeDefinition td, CodegenSession session, string methodName, MethodAttributes attributes, TypeReference returnType, out bool created)
        {
            MethodDefinition md = td.GetMethod(methodName);
            // Not found.
            if (md == null)
            {
                md = new(methodName, attributes, returnType);
                td.Methods.Add(md);
                created = true;
            }
            else
            {
                created = false;
            }

            return md.GetMethodReference(session);
        }

        /// <summary>
        /// Gets a MethodDefinition or creates one if missing.
        /// </summary>
        public static MethodDefinition GetOrCreateMethodDefinition(this TypeDefinition td, CodegenSession session, string methodName, MethodAttributes attributes, TypeReference returnType, out bool created)
        {
            MethodDefinition md = td.GetMethod(methodName);
            //Not found.
            if (md == null)
            {
                md = new(methodName, attributes, returnType);
                td.Methods.Add(md);
                created = true;
            }
            else
            {
                created = false;
            }

            return md;
        }

        /// <summary>
        /// Gets a MethodDefinition or creates one if missing.
        /// </summary>
        public static MethodDefinition GetOrCreateMethodDefinition(this TypeDefinition td, CodegenSession session, string methodName, MethodDefinition methodTemplate, bool copyParameters, out bool created)
        {
            MethodDefinition md = td.GetMethod(methodName);
            //Not found.
            if (md == null)
            {
                TypeReference returnType = session.ImportReference(methodTemplate.ReturnType);
                md = new(methodName, methodTemplate.Attributes, returnType)
                {
                    ExplicitThis = methodTemplate.ExplicitThis,
                    AggressiveInlining = methodTemplate.AggressiveInlining,
                    Attributes = methodTemplate.Attributes,
                    CallingConvention = methodTemplate.CallingConvention,
                    HasThis = methodTemplate.HasThis
                };
                md.Body.InitLocals = methodTemplate.Body.InitLocals;

                foreach (GenericParameter item in methodTemplate.GenericParameters)
                {
                    md.GenericParameters.Add(new(item.Name, md) { Attributes = item.Attributes });
                }

                if (copyParameters)
                {
                    foreach (ParameterDefinition pd in methodTemplate.Parameters)
                        md.Parameters.Add(pd.CloneImported(session, md));
                }

                td.Methods.Add(md);
                created = true;
            }
            else
            {
                created = false;
            }

            return md;
        }

        /// <summary>
        /// Returns a method in any inherited classes. The first found method is returned.
        /// </summary>
        public static MethodDefinition GetMethodDefinitionInAnyBase(this TypeDefinition td, CodegenSession session, string methodName)
        {
            while (td != null)
            {
                foreach (MethodDefinition md in td.Methods)
                {
                    if (md.Name == methodName)
                        return md;
                }

                try
                {
                    td = td.GetNextBaseTypeDefinition(session);
                }
                catch
                {
                    return null;
                }
            }

            return null;
        }

        /// <summary>
        /// Returns a TypeDefintiion found in typeDef or up it's hierarchy.
        /// </summary>
        /// <param name = "checkTypeDef">True to check if typeDef equals fullName.</param>
        /// <returns></returns>
        public static TypeDefinition GetTypeDefinitionInBase(this TypeDefinition typeDef, CodegenSession session, string targetFullName, bool checkTypeDef)
        {
            if (typeDef == null)
                return null;
            if (!checkTypeDef)
                typeDef = typeDef.GetNextBaseTypeDefinition(session);

            while (typeDef != null)
            {
                if (typeDef.FullName == targetFullName)
                    return typeDef;

                typeDef = typeDef.GetNextBaseTypeDefinition(session);
            }

            return null;
        }

        /// <summary>
        /// Returns the next base type.
        /// </summary>
        public static TypeDefinition GetNextBaseTypeDefinition(this TypeDefinition typeDef, CodegenSession session)
        {
            return typeDef.BaseType == null ? null : typeDef.BaseType.CachedResolve(session);
        }

        /// <summary>
        /// Creates a FieldReference.
        /// </summary>
        public static FieldReference CreateFieldReference(this FieldDefinition fd, CodegenSession session)
        {
            FieldReference fr;
            TypeDefinition declaringType = fd.DeclaringType;
            //Is generic.
            if (declaringType.HasGenericParameters)
            {
                GenericInstanceType git = new(declaringType);
                foreach (GenericParameter item in declaringType.GenericParameters)
                    git.GenericArguments.Add(item);
                fr = new(fd.Name, fd.FieldType, git);
                return fr;
            }
            //Not generic.
            else
            {
                return session.ImportReference(fd);
            }
        }

        /// <summary>
        /// Gets a FieldReference or creates it if missing.
        /// </summary>
        public static FieldReference GetOrCreateFieldReference(this TypeDefinition td, CodegenSession session, string fieldName, FieldAttributes attributes, TypeReference fieldTypeRef, out bool created)
        {
            FieldReference fr = td.GetFieldReference(fieldName, session);
            if (fr == null)
            {
                fr = td.CreateFieldDefinition(session, fieldName, attributes, fieldTypeRef);
                created = true;
            }
            else
            {
                created = false;
            }

            return fr;
        }

        /// <summary>
        /// Creates a FieldReference.
        /// </summary>
        public static FieldReference CreateFieldDefinition(this TypeDefinition td, CodegenSession session, string fieldName, FieldAttributes attributes, TypeReference fieldTypeRef)
        {
            FieldDefinition fd = new(fieldName, attributes, fieldTypeRef);
            td.Fields.Add(fd);
            return fd.CreateFieldReference(session);
        }

        /// <summary>
        /// Makes a GenericInstanceType.
        /// </summary>
        public static GenericInstanceType MakeGenericInstanceType(this TypeDefinition self, CodegenSession session)
        {
            TypeReference tr = session.ImportReference(self);
            return tr.MakeGenericInstanceType();
        }
    }
}