//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Collections.Generic;
using System;

namespace MonoFN.Cecil
{
    public interface IAssemblyResolver : IDisposable
    {
        AssemblyDefinition Resolve(AssemblyNameReference name);
        AssemblyDefinition Resolve(AssemblyNameReference name, ReaderParameters parameters);
    }

    public interface IMetadataResolver
    {
        TypeDefinition Resolve(TypeReference type);
        FieldDefinition Resolve(FieldReference field);
        MethodDefinition Resolve(MethodReference method);
    }

#if !NET_CORE
    [Serializable]
#endif
    public sealed class ResolutionException : Exception
    {
        public MemberReference Member { get; }
        public IMetadataScope Scope
        {
            get
            {
                TypeReference type = Member as TypeReference;
                if (type != null)
                    return type.Scope;

                TypeReference declaring_type = Member.DeclaringType;
                if (declaring_type != null)
                    return declaring_type.Scope;

                throw new NotSupportedException();
            }
        }

        public ResolutionException(MemberReference member) : base("Failed to resolve " + member.FullName)
        {
            if (member == null)
                throw new ArgumentNullException("member");

            this.Member = member;
        }

        public ResolutionException(MemberReference member, Exception innerException) : base("Failed to resolve " + member.FullName, innerException)
        {
            if (member == null)
                throw new ArgumentNullException("member");

            this.Member = member;
        }

#if !NET_CORE
        private ResolutionException(System.Runtime.Serialization.SerializationInfo info, System.Runtime.Serialization.StreamingContext context) : base(info, context) { }
#endif
    }

    public class MetadataResolver : IMetadataResolver
    {
        public IAssemblyResolver AssemblyResolver { get; }

        public MetadataResolver(IAssemblyResolver assemblyResolver)
        {
            if (assemblyResolver == null)
                throw new ArgumentNullException("assemblyResolver");

            AssemblyResolver = assemblyResolver;
        }

        public virtual TypeDefinition Resolve(TypeReference type)
        {
            Mixin.CheckType(type);

            type = type.GetElementType();

            IMetadataScope scope = type.Scope;

            if (scope == null)
                return null;

            switch (scope.MetadataScopeType)
            {
                case MetadataScopeType.AssemblyNameReference:
                    AssemblyDefinition assembly = AssemblyResolver.Resolve((AssemblyNameReference)scope);
                    if (assembly == null)
                        return null;

                    return GetType(assembly.MainModule, type);
                case MetadataScopeType.ModuleDefinition:
                    return GetType((ModuleDefinition)scope, type);
                case MetadataScopeType.ModuleReference:
                    if (type.Module.Assembly == null)
                        return null;

                    Collection<ModuleDefinition> modules = type.Module.Assembly.Modules;
                    ModuleReference module_ref = (ModuleReference)scope;
                    for (int i = 0; i < modules.Count; i++)
                    {
                        ModuleDefinition netmodule = modules[i];
                        if (netmodule.Name == module_ref.Name)
                            return GetType(netmodule, type);
                    }
                    break;
            }

            throw new NotSupportedException();
        }

        private static TypeDefinition GetType(ModuleDefinition module, TypeReference reference)
        {
            TypeDefinition type = GetTypeDefinition(module, reference);
            if (type != null)
                return type;

            if (!module.HasExportedTypes)
                return null;

            Collection<ExportedType> exported_types = module.ExportedTypes;

            for (int i = 0; i < exported_types.Count; i++)
            {
                ExportedType exported_type = exported_types[i];
                if (exported_type.Name != reference.Name)
                    continue;

                if (exported_type.Namespace != reference.Namespace)
                    continue;

                return exported_type.Resolve();
            }

            return null;
        }

        private static TypeDefinition GetTypeDefinition(ModuleDefinition module, TypeReference type)
        {
            if (!type.IsNested)
                return module.GetType(type.Namespace, type.Name);

            TypeDefinition declaring_type = type.DeclaringType.Resolve();
            if (declaring_type == null)
                return null;

            return declaring_type.GetNestedType(type.TypeFullName());
        }

        public virtual FieldDefinition Resolve(FieldReference field)
        {
            Mixin.CheckField(field);

            TypeDefinition type = Resolve(field.DeclaringType);
            if (type == null)
                return null;

            if (!type.HasFields)
                return null;

            return GetField(type, field);
        }

        private FieldDefinition GetField(TypeDefinition type, FieldReference reference)
        {
            while (type != null)
            {
                FieldDefinition field = GetField(type.Fields, reference);
                if (field != null)
                    return field;

                if (type.BaseType == null)
                    return null;

                type = Resolve(type.BaseType);
            }

            return null;
        }

        private static FieldDefinition GetField(Collection<FieldDefinition> fields, FieldReference reference)
        {
            for (int i = 0; i < fields.Count; i++)
            {
                FieldDefinition field = fields[i];

                if (field.Name != reference.Name)
                    continue;

                if (!AreSame(field.FieldType, reference.FieldType))
                    continue;

                return field;
            }

            return null;
        }

        public virtual MethodDefinition Resolve(MethodReference method)
        {
            Mixin.CheckMethod(method);

            TypeDefinition type = Resolve(method.DeclaringType);
            if (type == null)
                return null;

            method = method.GetElementMethod();

            if (!type.HasMethods)
                return null;

            return GetMethod(type, method);
        }

        private MethodDefinition GetMethod(TypeDefinition type, MethodReference reference)
        {
            while (type != null)
            {
                MethodDefinition method = GetMethod(type.Methods, reference);
                if (method != null)
                    return method;

                if (type.BaseType == null)
                    return null;

                type = Resolve(type.BaseType);
            }

            return null;
        }

        public static MethodDefinition GetMethod(Collection<MethodDefinition> methods, MethodReference reference)
        {
            for (int i = 0; i < methods.Count; i++)
            {
                MethodDefinition method = methods[i];

                if (method.Name != reference.Name)
                    continue;

                if (method.HasGenericParameters != reference.HasGenericParameters)
                    continue;

                if (method.HasGenericParameters && method.GenericParameters.Count != reference.GenericParameters.Count)
                    continue;

                if (!AreSame(method.ReturnType, reference.ReturnType))
                    continue;

                if (method.IsVarArg() != reference.IsVarArg())
                    continue;

                if (method.IsVarArg() && IsVarArgCallTo(method, reference))
                    return method;

                if (method.HasParameters != reference.HasParameters)
                    continue;

                if (!method.HasParameters && !reference.HasParameters)
                    return method;

                if (!AreSame(method.Parameters, reference.Parameters))
                    continue;

                return method;
            }

            return null;
        }

        private static bool AreSame(Collection<ParameterDefinition> a, Collection<ParameterDefinition> b)
        {
            int count = a.Count;

            if (count != b.Count)
                return false;

            if (count == 0)
                return true;

            for (int i = 0; i < count; i++)
                if (!AreSame(a[i].ParameterType, b[i].ParameterType))
                    return false;

            return true;
        }

        private static bool IsVarArgCallTo(MethodDefinition method, MethodReference reference)
        {
            if (method.Parameters.Count >= reference.Parameters.Count)
                return false;

            if (reference.GetSentinelPosition() != method.Parameters.Count)
                return false;

            for (int i = 0; i < method.Parameters.Count; i++)
                if (!AreSame(method.Parameters[i].ParameterType, reference.Parameters[i].ParameterType))
                    return false;

            return true;
        }

        private static bool AreSame(TypeSpecification a, TypeSpecification b)
        {
            if (!AreSame(a.ElementType, b.ElementType))
                return false;

            if (a.IsGenericInstance)
                return AreSame((GenericInstanceType)a, (GenericInstanceType)b);

            if (a.IsRequiredModifier || a.IsOptionalModifier)
                return AreSame((IModifierType)a, (IModifierType)b);

            if (a.IsArray)
                return AreSame((ArrayType)a, (ArrayType)b);

            return true;
        }

        private static bool AreSame(ArrayType a, ArrayType b)
        {
            if (a.Rank != b.Rank)
                return false;

            // TODO: dimensions

            return true;
        }

        private static bool AreSame(IModifierType a, IModifierType b)
        {
            return AreSame(a.ModifierType, b.ModifierType);
        }

        private static bool AreSame(GenericInstanceType a, GenericInstanceType b)
        {
            if (a.GenericArguments.Count != b.GenericArguments.Count)
                return false;

            for (int i = 0; i < a.GenericArguments.Count; i++)
                if (!AreSame(a.GenericArguments[i], b.GenericArguments[i]))
                    return false;

            return true;
        }

        private static bool AreSame(GenericParameter a, GenericParameter b)
        {
            return a.Position == b.Position;
        }

        private static bool AreSame(TypeReference a, TypeReference b)
        {
            if (ReferenceEquals(a, b))
                return true;

            if (a == null || b == null)
                return false;

            if (a.etype != b.etype)
                return false;

            if (a.IsGenericParameter)
                return AreSame((GenericParameter)a, (GenericParameter)b);

            if (a.IsTypeSpecification())
                return AreSame((TypeSpecification)a, (TypeSpecification)b);

            if (a.Name != b.Name || a.Namespace != b.Namespace)
                return false;

            return AreSame(a.DeclaringType, b.DeclaringType);
        }
    }
}