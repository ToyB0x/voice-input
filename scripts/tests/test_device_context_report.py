from copy import deepcopy
import unittest
from device_context_report import merge

class DeviceReports(unittest.TestCase):
    def report(self): return dict(kind='device-context-lab',schemaVersion=1,reportId='a',includesText=True,completed=True,phase='done',totalRows=3,rowOffset=0,rows=[{'i':0},{'i':1},{'i':2}])
    def test_full_and_split_reports_preserve_actual_rows(self):
        r=self.report();self.assertEqual(merge([r])['rows'],r['rows'])
        a,b=deepcopy(r),deepcopy(r);a['rows']=a['rows'][:2];b['rows']=b['rows'][2:];b['rowOffset']=2
        self.assertEqual(merge([b,a])['rows'],r['rows'])
        for parts in [[a],[a,a]]:
            with self.assertRaises(ValueError): merge(parts)
        b['reportId']='different'
        with self.assertRaises(ValueError): merge([a,b])
    def test_redacted_cancelled_and_oversized_counts_are_not_evidence(self):
        for key,value in [('includesText',False),('completed',False),('phase','cancelled'),('totalRows',999999)]:
            r=self.report();r[key]=value
            with self.assertRaises(ValueError): merge([r])
